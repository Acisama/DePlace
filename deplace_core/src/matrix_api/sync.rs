use std::collections::HashMap;
use std::pin::pin;
use std::time::Duration;

use futures_util::StreamExt;
use matrix_sdk::{Client, config::SyncSettings, ruma::presence::PresenceState};

use crate::state::AppState;

/// Starts the background sync loop and the room classification loop for `state`.
pub fn spawn_room_sync(client: &Client, state: &AppState) {
    tokio::spawn(run_sync_stream(client.clone()));
    tokio::spawn(run_room_classification(client.clone(), state.clone()));
}

async fn run_sync_stream(client: Client) {
    let sync_settings = SyncSettings::default()
        .ignore_timeout_on_first_sync(true)
        .set_presence(PresenceState::Online)
        .timeout(Duration::from_secs(30));

    let sync_stream = client.sync_stream(sync_settings).await;
    let mut sync_stream = pin!(sync_stream);

    while let Some(result) = sync_stream.next().await {
        if let Err(e) = result {
            tracing::error!("Sync loop returned an error: {e:?}");
        }
    }

    tracing::warn!("Sync stream ended");
}

/// Classifies every room into `dm_rooms`/`server_rooms` once at startup, then
/// re-classifies all rooms from scratch whenever any room's info notably changes.
/// `AppState::set_dm_rooms`/`set_server_rooms` already skip the `watch` send when
/// the resulting set of room IDs didn't actually change, so this stays cheap even
/// though it's a full rescan rather than an incremental patch.
async fn run_room_classification(client: Client, state: AppState) {
    reclassify_rooms(&client, &state).await;

    let mut updates = client.room_info_notable_update_receiver();
    while updates.recv().await.is_ok() {
        reclassify_rooms(&client, &state).await;
    }
}

async fn reclassify_rooms(client: &Client, state: &AppState) {
    let mut dm_rooms = HashMap::new();
    let mut server_rooms = HashMap::new();

    for room in client.rooms() {
        let is_dm = match room.compute_is_dm().await {
            Ok(is_dm) => is_dm,
            Err(e) => {
                tracing::error!("Failed to compute is_dm for room {}: {e}", room.room_id());
                false
            }
        };

        if is_dm {
            dm_rooms.insert(room.room_id().to_owned(), room);
        } else if room.is_space() {
            server_rooms.insert(room.room_id().to_owned(), room);
        }
    }

    state.set_dm_rooms(dm_rooms);
    state.set_server_rooms(server_rooms);
}
