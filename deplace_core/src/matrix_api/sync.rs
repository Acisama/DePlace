use std::collections::HashSet;
use std::{
    pin::pin,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::audio::AudioThread;
use crate::call::play_signature_samples;
use crate::media::ProfileAvatars;
use crate::member::ProfileStore;
use crate::member::synth::SignatureEvent;
use crate::member::user_data::handle_presence_update;
use crate::room::call::{CallMember, CallMembers};
use crate::room::notifications::NotificationCounts;
use crate::state::UserDevice;
use crate::{
    matrix_api::{hierarchy::run_space_hierarchy_update, save_session},
    notifications::on_message,
    state::AppState,
};
use futures_util::StreamExt;
use matrix_sdk::{Client, config::SyncSettings, ruma::presence::PresenceState, sync::SyncResponse};
use tokio::sync::mpsc;

use super::matrixrtc::{cleanup_ghost_calls, handle_call_member_change, handle_to_device_messages};
use super::members::handle_member_event;

pub fn spawn_sync_runners(client: &Client, state: &AppState, own_device: Arc<UserDevice>) {
    // The last session may have died without withdrawing our call
    // memberships, ghosting us into calls: other members keep seeing
    // and waiting for us long after the app is gone. Whatever is left
    // behind is cleaned up here, before it can mislead anyone else.
    tokio::spawn({
        let client = client.clone();
        async move { cleanup_ghost_calls(&client).await }
    });

    tokio::spawn(run_sync_stream(client.clone(), state.clone()));
    tokio::spawn(run_sync_stall_watchdog());
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

/// Time of the last processed sync response, zero means none arrived yet.
static LAST_SYNC: AtomicU64 = AtomicU64::new(0);

/// Warn when no sync response arrived for this long.
const SYNC_STALL_WARN_AFTER: u64 = 90;

/// Hard deadline for one fetch from the sync stream: the 30 s long poll plus
/// one SDK retry. Rebuilding is cheap now that a wedged stream is aborted.
const SYNC_FETCH_DEADLINE: Duration = Duration::from_secs(60);

/// Hard deadline for handling one sync response
const SYNC_HANDLE_DEADLINE: Duration = Duration::from_secs(120);

/// Pause between sync stream rebuilds
const SYNC_REBUILD_PAUSE: Duration = Duration::from_secs(2);

async fn run_sync_stream(client: Client, state: AppState) {
    let sync_settings = SyncSettings::default()
        .ignore_timeout_on_first_sync(true)
        .set_presence(PresenceState::Online)
        .timeout(Duration::from_secs(30));

    let mut responses = 0u64;
    let mut restarts = 0u64;
    let mut handler_attached = false;

    let (handle_tx, handle_rx) = mpsc::channel::<SyncResponse>(4);
    spawn_sync_handler(state.clone(), handle_rx);

    // Outer loop: whenever the stream stalls or ends, a fresh one is
    // rebuilt from the client's stored sync token, so message reception
    // recovers on its own instead of requiring an app restart.
    loop {
        tracing::debug!(restarts, responses, "Starting the sync stream");

        // The stream is pulled on its own detached task and the
        // responses forwarded through a channel: if pulling ever wedges
        // inside a poll (blocking call in the SDK), only the puller task
        // dies unreachable -- this loop stays at an await on the channel
        // receiver, so the fetch deadline below can still fire and
        // rebuild the stream. Pulling on the loop's own task took that
        // safety away: a wedged poll parks this loop's worker inside
        // `next()`, no timer can reach it anymore, and the fetch
        // deadline never fires -- the loop is dead without a trace
        // while the watchdog keeps shouting into the void.
        let (response_tx, mut response_rx) = mpsc::channel::<matrix_sdk::Result<SyncResponse>>(1);
        // The stream borrows its `Client`, so it is created inside the task
        // from a clone the task owns. The handle is kept so that a stream
        // wedged inside a poll can be aborted below: dropping the receiver
        // only makes it stop *after* its next response, which a dead stream
        // never produces -- the rebuild would then wait behind a sync that
        // never finishes, and message reception stayed dead for as long.
        let puller_client = client.clone();
        let puller_settings = sync_settings.clone();
        let puller = tokio::spawn(async move {
            let sync_stream = puller_client.sync_stream(puller_settings).await;
            let mut sync_stream = pin!(sync_stream);
            while let Some(result) = sync_stream.next().await {
                #[allow(clippy::useless_conversion)]
                if response_tx.send(result.into()).await.is_err() {
                    // The loop dropped the receiver and moved on to a
                    // fresh stream; stop pulling this one.
                    break;
                }
            }
        });

        loop {
            let fetched = tokio::time::timeout(SYNC_FETCH_DEADLINE, response_rx.recv()).await;

            let result = match fetched {
                Ok(Some(result)) => result,
                Ok(None) => {
                    tracing::error!(
                        responses,
                        restarts = restarts + 1,
                        "Sync stream ended after {responses} responses; it is rebuilt so messages keep flowing"
                    );
                    break;
                }
                Err(_elapsed) => {
                    tracing::error!(
                        responses,
                        restarts = restarts + 1,
                        "Sync stream stalled for {SYNC_FETCH_DEADLINE:?} without a response, rebuilding it to squash the dead sync bug"
                    );
                    break;
                }
            };

            responses += 1;

            // Noted on arrival, not once handling finished: the watchdog
            // reports the stream's health, and a slow handler is not a stall.
            note_sync_received();

            // Handled on its own task and queue, so a handler that blocks can
            // never hold up the next fetch.
            match result {
                Ok(response) => {
                    if handle_tx.send(response).await.is_err() {
                        tracing::error!("Sync handler is gone, rebuilding the stream");
                        break;
                    }
                }
                Err(e) => tracing::error!("Sync loop returned an error: {e:?}"),
            }

            if !handler_attached {
                client.add_event_handler(on_message);
                handler_attached = true;
                tracing::debug!("First sync response processed, message handler attached");
            }
        }

        // A stalled stream never yields again, so it has to be aborted
        // instead of left behind: a rebuild would otherwise wait for a sync
        // that never returns, and no message would arrive at all.
        puller.abort();

        restarts += 1;
        tokio::time::sleep(SYNC_REBUILD_PAUSE).await;
    }
}

/// Applies sync responses one at a time, in arrival order.
///
/// Kept off the stream loop, so a blocking handler delays only the responses
/// queued behind it.
fn spawn_sync_handler(state: AppState, mut responses: mpsc::Receiver<SyncResponse>) {
    tokio::spawn(async move {
        while let Some(response) = responses.recv().await {
            let state = state.clone();
            let handler = tokio::spawn(async move { handle_sync_result(response, &state).await });

            match tokio::time::timeout(SYNC_HANDLE_DEADLINE, handler).await {
                Ok(Ok(())) => {}
                Ok(Err(join_error)) => {
                    tracing::error!(
                        "Handling a sync response panicked \
                         ({join_error:?}); the sync stream itself stays alive"
                    );
                }
                Err(_elapsed) => {
                    tracing::error!(
                        "Handling a sync response stalled for \
                         {SYNC_HANDLE_DEADLINE:?}; the next ones are handled \
                         anyway, so the stream keeps its pace"
                    );
                }
            }
        }
    });
}

/// Watches [`LAST_SYNC`] and warns while no sync response arrives, so a
/// stalled sync loop stays visible in the logs. The stream loop rebuilds
/// itself on [`SYNC_FETCH_DEADLINE`], this only reports the outage.
async fn run_sync_stall_watchdog() {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;

        let last = LAST_SYNC.load(Ordering::Relaxed);
        if last == 0 {
            continue;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since_epoch| since_epoch.as_secs())
            .unwrap_or(0);
        let quiet = now.saturating_sub(last);
        if quiet >= SYNC_STALL_WARN_AFTER {
            tracing::warn!("No sync response for {quiet}s, message reception looks stalled");
        }
    }
}

fn note_sync_received() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_secs())
        .unwrap_or(0);
    LAST_SYNC.store(now, Ordering::Relaxed);
}

/// Time of the last own-profile refresh, zero means never.
static LAST_PROFILE_REFRESH: AtomicU64 = AtomicU64::new(0);

/// How often our own display name and avatar are re-fetched.
const PROFILE_REFRESH_INTERVAL: u64 = 300;

fn own_profile_is_stale() -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since_epoch| since_epoch.as_secs())
        .unwrap_or(0);

    let last = LAST_PROFILE_REFRESH.load(Ordering::Relaxed);
    if now.saturating_sub(last) < PROFILE_REFRESH_INTERVAL {
        return false;
    }

    LAST_PROFILE_REFRESH.store(now, Ordering::Relaxed);

    true
}

async fn handle_sync_result(result: SyncResponse, state: &AppState) {
    let timeline_events: usize = result
        .rooms
        .joined
        .values()
        .map(|room| room.timeline.events.len())
        .sum();
    tracing::debug!(
        "Sync response: next_batch={} joined_updates={} timeline_events={} to_device_events={}",
        result.next_batch,
        result.rooms.joined.len(),
        timeline_events,
        result.to_device.len(),
    );

    // Call encryption keys arrive as decrypted to-device events
    handle_to_device_messages(result.to_device, state.livekit_room_manager()).await;

    // Only every few minutes: each is its own HTTP request, and refreshing per
    // response added two round trips to the sync path.
    if own_profile_is_stale() {
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
                    state.inner.own_avatar.send_replace(original.map(|u| {
                        ProfileAvatars::get(
                            state.image_cache(),
                            u,
                            state.own_device_ref().user_id.as_str(),
                        )
                    }));
                }
            }
            Err(e) => {
                tracing::error!("Failed to get own avatar url: {}", e);
            }
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
            // it fresh - nothing to sync yet. But if it is a DM it has to
            // enter the list right away
            if created {
                let belongs_in_dm_list = client
                    .get_room(&room_id)
                    .is_some_and(|room| state.room_store_ref().belongs_in_home_list(&room));
                if belongs_in_dm_list {
                    state.room_store_ref().recompute_home();
                }
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
                        info.update_avatar(avatar_url.clone(), state.image_cache());
                    }
                });
            }

            // RECENCYSTAMP is presumably only set by the new sliding sync
            // which we don't use yet, so until then we have to stay with
            // our old logic
            if state.room_store_ref().belongs_in_home_list(&room) {
                state.room_store_ref().recompute_home();
            }

            let notifications: NotificationCounts = room.unread_notification_counts().into();

            // The open chat marks messages read as soon as they render, and the
            // server keeps reporting counts until it has caught up with our
            // receipt, so those must not pop a badge up in the meantime.
            let watching = state.window_focused() && state.is_active_room(&room_id);
            let already_seen = room.latest_event().event_id().is_some_and(|newest| {
                state
                    .room_seen_marker(&room_id)
                    .is_some_and(|seen| seen == newest)
            });

            let notifications = if watching || already_seen {
                NotificationCounts::default()
            } else {
                notifications
            };

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
