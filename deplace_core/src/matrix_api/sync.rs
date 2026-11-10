use std::collections::HashSet;
use std::{pin::pin, sync::Arc};

use crate::audio::AudioThread;
use crate::cache::ProfileAvatars;
use crate::call::play_signature_samples;
use crate::member::ProfileStore;
use crate::member::synth::SignatureEvent;
use crate::member::user_data::handle_presence_update;
use crate::room::call::{CallMember, CallMembers};
use crate::state::UserDevice;
use crate::{
    matrix_api::{hierarchy::run_space_hierarchy_update, save_session},
    notifications::on_message,
    state::AppState,
};
use futures_util::StreamExt;
use matrix_sdk::{Client, config::SyncSettings, ruma::presence::PresenceState, sync::SyncResponse};
use matrix_sdk_base::RoomInfoNotableUpdateReasons;

use super::matrixrtc::{cleanup_ghost_calls, handle_call_member_change, handle_to_device_messages};
use super::members::handle_member_event;

pub fn spawn_sync_runners(client: &Client, state: &AppState, own_device: Arc<UserDevice>) {
    if let Err(e) = client.event_cache().subscribe() {
        tracing::error!("Failed to subscribe to event cache: {e}");
    }

    // The last session may have died without withdrawing our call
    // memberships, ghosting us into calls: other members keep seeing
    // and waiting for us long after the app is gone. Whatever is left
    // behind is cleaned up here, before it can mislead anyone else.
    tokio::spawn({
        let client = client.clone();
        async move { cleanup_ghost_calls(&client).await }
    });

    tokio::spawn(run_sync_stream(client.clone(), state.clone()));
    tokio::spawn(run_keystore_save_stream(client.clone()));
    spawn_room_sync(client.clone(), state.clone(), own_device.clone());
    run_space_hierarchy_update(client);

    client.add_event_handler_context(state.clone());
    client.add_event_handler(handle_member_event);
    client.add_event_handler(handle_presence_update);
    client.add_event_handler(handle_call_member_change);
}

/// Which members are in `new` but not `old`, and which are in `old` but
/// not `new` — i.e. who joined and who left the call's Matrix roster.
fn call_members_diff<'a>(
    old: &'a CallMembers,
    new: &'a CallMembers,
) -> (Vec<&'a CallMember>, Vec<&'a CallMember>) {
    let joined = new
        .members
        .iter()
        .filter(|m| !old.members.iter().any(|o| o == *m))
        .collect();
    let left = old
        .members
        .iter()
        .filter(|m| !new.members.iter().any(|n| n == *m))
        .collect();
    (joined, left)
}

/// Plays join/left sounds for whatever actually changed between two
/// `CallMembers` snapshots (including our own join/leave), so they fire in
/// step with what the UI's participant list shows rather than LiveKit's
/// own connection events.
///
/// Gated on the Matrix roster itself (not on whether LiveKit has caught up
/// yet): our own membership state event and our LiveKit connection aren't
/// ordered relative to each other, so waiting for LiveKit to confirm we're
/// in the call could miss our own join/leave entirely.
fn play_call_members_diff_sounds(audio: &AudioThread, old: &CallMembers, new: &CallMembers) {
    if !old.own_user_device_is_in_call() && !new.own_user_device_is_in_call() {
        return;
    }

    let (joined, left) = call_members_diff(old, new);

    for member in joined {
        play_signature_samples(
            audio,
            member.calc_synth_sound(SignatureEvent::Joined),
            SignatureEvent::Joined,
        );
    }
    for member in left {
        play_signature_samples(
            audio,
            member.calc_synth_sound(SignatureEvent::Left),
            SignatureEvent::Left,
        );
    }
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
    let result = match result {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Sync loop returned an error: {e:?}");
            return;
        }
    };

    // Call encryption keys arrive as decrypted to-device events
    handle_to_device_messages(result.to_device, state.livekit_room_manager()).await;

    let account = state.client().account();

    match account.get_display_name().await {
        Ok(name) => state.set_own_display_name(name),
        Err(e) => {
            tracing::error!("Failed to get own display name: {}", e);
        }
    }

    match account.get_avatar_url().await {
        Ok(original) => {
            let url = original.clone().map(Arc::new);
            let changed = state.inner.own_avatar_url.borrow().as_ref() != url.as_ref();
            state.inner.own_avatar_url.send_replace(url);

            if changed {
                state.inner.own_avatar.send_replace(
                    original.map(|u| ProfileAvatars::get(state.thumbnail_cache(), u)),
                );
            }
        }
        Err(e) => {
            tracing::error!("Failed to get own avatar url: {}", e);
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
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Room update receiver lagged, skipped {n} updates");
                    continue;
                }
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
                        info.update_avatar(avatar_url.clone(), state.thumbnail_cache());
                    }
                });
            }

            if room.is_dm()
                && update
                    .reasons
                    .intersects(RoomInfoNotableUpdateReasons::RECENCY_STAMP)
            {
                state.room_store_ref().recompute_dms();
            }

            let notifications = room.unread_notification_counts().into();

            dp_room.update_live_if_modified(|live| {
                live.notification_counts = notifications;
            });
            let old_members = dp_room.call.members.get();
            let new_members =
                CallMembers::new(&room, profile_store, &dp_room.call, &own_device).await;
            if let Some(audio) = state.audio() {
                play_call_members_diff_sounds(&audio, &old_members, &new_members);
            }
            dp_room.call.members.set_if_modified(new_members);
        }
    });
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

            dp_room.call.members.set_if_modified(
                CallMembers::new(&room, &profile_store, &dp_room.call, &own_device).await,
            );
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

        profile_store.recalculate_all_rooms();
    });
}
