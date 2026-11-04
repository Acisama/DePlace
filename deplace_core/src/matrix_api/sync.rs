use std::collections::HashSet;
use std::{pin::pin, sync::Arc};

use futures_util::StreamExt;
use indexmap::IndexSet;
use matrix_sdk::{
    Client, Room, config::SyncSettings, deserialized_responses::SyncOrStrippedState,
    ruma::presence::PresenceState, sync::SyncResponse,
};
use matrix_sdk_base::RoomInfoNotableUpdateReasons;
use ruma::RoomId;
use ruma::events::call::member::CallMemberEventContent;

use crate::member::ProfileStore;
use crate::member::user_data::handle_presence_update;
use crate::room::live::CallMember;
use crate::state::UserDevice;
use crate::thumbnail_format;
use crate::{
    matrix_api::{hierarchy::run_space_hierarchy_update, save_session},
    notifications::on_message,
    state::AppState,
};

#[cfg(feature = "iced")]
use crate::Avatar;
#[cfg(feature = "iced")]
use iced::advanced::image::Handle as ImageHandle;

use super::members::handle_member_event;

pub fn spawn_sync_runners(client: &Client, state: &AppState, own_device: Arc<UserDevice>) {
    if let Err(e) = client.event_cache().subscribe() {
        tracing::error!("Failed to subscribe to event cache: {e}");
    }

    tokio::spawn(run_sync_stream(client.clone(), state.clone()));
    tokio::spawn(run_keystore_save_stream(client.clone()));
    spawn_room_sync(client.clone(), state.clone(), own_device.clone());
    run_space_hierarchy_update(client);

    client.add_event_handler_context(state.clone());
    client.add_event_handler(handle_member_event);
    client.add_event_handler(handle_presence_update);
}

async fn run_keystore_save_stream(client: Client) {
    save_session(&client);

    let mut updates = client.subscribe_to_session_changes();
    while updates.recv().await.is_ok() {
        save_session(&client);
    }
}

async fn run_sync_stream(client: Client, state: AppState) {
    let sync_settings = SyncSettings::default()
        .ignore_timeout_on_first_sync(true)
        .set_presence(PresenceState::Online)
        .timeout(std::time::Duration::from_secs(30));

    let sync_stream = client.sync_stream(sync_settings).await;
    let mut sync_stream = pin!(sync_stream);

    let Some(result) = sync_stream.next().await else {
        return;
    };

    #[allow(clippy::useless_conversion)]
    handle_sync_result(result.into(), &state).await;
    client.add_event_handler(on_message);

    while let Some(result) = sync_stream.next().await {
        #[allow(clippy::useless_conversion)]
        handle_sync_result(result.into(), &state).await;
    }

    tracing::warn!("Sync stream ended");
}

async fn handle_sync_result(result: matrix_sdk::Result<SyncResponse>, state: &AppState) {
    let _result = match result {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Sync loop returned an error: {e:?}");
            return;
        }
    };

    let account = state.client().account();

    match account.get_display_name().await {
        Ok(name) => state.set_own_display_name(name),
        Err(e) => {
            tracing::error!("Failed to get own display name: {}", e);
        }
    }

    match account.get_avatar_url().await {
        Ok(url) => {
            state.inner.own_avatar_url.send_replace(url.map(Arc::new));
        }
        Err(e) => {
            tracing::error!("Failed to get own avatar url: {}", e);
        }
    }

    #[cfg(feature = "iced")]
    match account.get_avatar(thumbnail_format()).await {
        Ok(Some(bytes)) if let Some(url) = state.inner.own_avatar_url.borrow().clone() => {
            state.inner.own_avatar_handle.send_replace(Avatar::Ok {
                image: ImageHandle::from_bytes(bytes),
                avatar_url: url,
            });
        }
        Ok(None | _) => {
            state.inner.own_avatar_handle.send_replace(Avatar::NoAvatar);
        }
        Err(e) => {
            tracing::error!("Failed to get own avatar: {}", e);
            state
                .inner
                .own_avatar_handle
                .send_replace(Avatar::Error(Arc::new(e.to_string())));
        }
    }

    state.bump_sync_tick();
}

fn spawn_room_sync(client: Client, state: AppState, own_device: Arc<UserDevice>) {
    let mut receiver = client.room_info_notable_update_receiver();

    tokio::spawn(async move {
        let profile_store = state.profile_store_ref();

        loop {
            let update = match receiver.recv().await {
                Ok(update) => update,
                Err(e) => {
                    tracing::error!("Failed to receive room update: {e:?}");
                    break;
                }
            };

            let room_id = update.room_id;

            let Some((dp_room, created)) =
                state.room_store_ref().get_room_with_if_created(&room_id)
            else {
                continue;
            };

            // Room was just created by the lookup above, which already loads
            // it fresh - nothing to sync yet.
            if created {
                continue;
            }

            let Some(room) = client.get_room(&room_id) else {
                continue;
            };

            let display_name = room.cached_display_name().map(|n| n.to_string());
            let avatar_url = room.avatar_url();

            let displayname_changed =
                dp_room.info.data.load().display_name.as_ref() != display_name.as_ref();
            let avatar_url_changed =
                dp_room.info.data.load().avatar_url.as_ref() != avatar_url.as_ref();

            if displayname_changed || avatar_url_changed {
                dp_room.update_info(|info| {
                    if displayname_changed {
                        info.display_name = display_name.clone();
                        info.calculate_strings();
                    }
                    if avatar_url_changed {
                        info.avatar_url = avatar_url.clone();
                    }
                });

                #[cfg(feature = "iced")]
                if avatar_url_changed {
                    state.room_store_ref().spawn_avatar_load(dp_room.clone());
                }
            }

            if room.is_dm()
                && update
                    .reasons
                    .intersects(RoomInfoNotableUpdateReasons::RECENCY_STAMP)
            {
                state.room_store_ref().recompute_dms();
            }

            let notifications = room.unread_notification_counts().into();
            let call_participants = compute_call_participants(&room, &room_id, profile_store).await;

            dp_room.update_live_if_modified(|live| {
                live.notification_counts = notifications;
                live.call_participants = call_participants.clone();
                live.calculate_own_call(&own_device);
            });
        }
    });
}

async fn compute_call_participants(
    room: &Room,
    room_id: &RoomId,
    profile_store: &ProfileStore,
) -> IndexSet<CallMember> {
    let mut call_participants = IndexSet::new();

    if room.has_active_room_call() {
        let events = match room
            .get_state_events_static::<CallMemberEventContent>()
            .await
        {
            Ok(events) => events,
            Err(e) => {
                tracing::error!("Failed to get call member events: {e}");
                Vec::new()
            }
        };

        for raw in events {
            let Ok(SyncOrStrippedState::Sync(ev)) = raw.deserialize() else {
                continue;
            };
            let matrix_sdk::ruma::events::SyncStateEvent::Original(ev) = ev else {
                continue;
            };
            let member = profile_store.get_member(room_id, ev.state_key.user_id());

            for membership in ev.content.active_memberships(None) {
                call_participants.insert(CallMember::new(&membership, member.clone()));
            }
        }
    }

    call_participants.sort_by_cached_key(|p| {
        (
            p.member.raw_display_name_lowercase().unwrap_or_default(),
            p.member.user_id.clone(),
        )
    });

    call_participants
}

pub(crate) fn spawn_initial_load(
    profile_store: Arc<ProfileStore>,
    client: Client,
    room_store: Arc<crate::room::store::RoomStore>,
    own_device: Arc<UserDevice>,
) {
    tokio::spawn(async move {
        let mut user_ids = HashSet::new();

        for room in client.rooms() {
            user_ids.extend(profile_store.load_room_members(&room).await);

            if !room.has_active_room_call() {
                continue;
            }

            let Some(dp_room) = room_store.get_room(room.room_id()) else {
                continue;
            };

            let call_participants =
                compute_call_participants(&room, room.room_id(), &profile_store).await;

            dp_room.update_live_if_modified(|live| {
                live.call_participants = call_participants.clone();
                live.calculate_own_call(&own_device);
            });
        }

        let user_ids: Vec<_> = user_ids.into_iter().collect();

        if let Ok(presences) = client
            .state_store()
            .get_presence_events(&user_ids)
            .await
            .map_err(|e| tracing::error!("Failed to get initial event states: {e}"))
        {
            for (id, content) in presences.iter().filter_map(|raw_ev| {
                let ev = match raw_ev.deserialize() {
                    Ok(ev) => ev,
                    Err(_) => return None,
                };
                Some((ev.sender.clone(), ev.content.clone()))
            }) {
                let (_, cell) = profile_store.user_cell(&id);
                cell.change(|data| {
                    data.update_from_content(content.clone());
                });
            }
        }
    });
}
