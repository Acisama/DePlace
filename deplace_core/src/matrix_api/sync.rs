use std::collections::HashMap;
use std::pin::pin;
use std::time::Duration;

use futures_util::StreamExt;
use matrix_sdk::{
    Client, Room, config::SyncSettings, room::ParentSpace, ruma::presence::PresenceState,
};
use ruma::OwnedRoomId;

use crate::{
    matrix_api::account_data::{ServerOrderContent, get_account_data},
    state::AppState,
};

pub fn spawn_room_sync(client: &Client, state: &AppState) {
    tokio::spawn(run_sync_stream(client.clone()));
    tokio::spawn(run_room_classification(client.clone(), state.clone()));
    tokio::spawn(init_stuff(client.clone(), state.clone()));
}

async fn init_stuff(client: Client, state: AppState) {
    if let Err(e) = state
        .server_order
        .send(get_account_data::<ServerOrderContent>(&client).await)
    {
        tracing::error!("Failed to send server order: {}", e);
    }
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
    let mut single_rooms = HashMap::new();

    let mut parent_to_children: HashMap<OwnedRoomId, Vec<Room>> = HashMap::new();
    let mut child_to_parents: HashMap<OwnedRoomId, Vec<Room>> = HashMap::new();

    let rooms = client.rooms();
    for room in rooms {
        let parents = match room.parent_spaces().await {
            Ok(parents) => parents,
            Err(e) => {
                tracing::error!(
                    "Failed to get parent spaces for room {}: {e}",
                    room.room_id()
                );
                continue;
            }
        };

        let parents_res = parents.collect::<Vec<_>>().await;

        let parents: Vec<Room> = parents_res
            .iter()
            .filter_map(|res| {
                if let Ok(ParentSpace::Reciprocal(room)) = res {
                    Some(room.clone())
                } else {
                    None
                }
            })
            .collect();

        child_to_parents.insert(room.room_id().to_owned(), parents.clone());

        for parent in parents {
            let entry = parent_to_children
                .entry(parent.room_id().to_owned())
                .or_default();
            entry.push(parent.clone());
        }
    }

    for room in client.rooms() {
        let room_id = room.room_id().to_owned();

        let is_dm = match room.compute_is_dm().await {
            Ok(is_dm) => is_dm,
            Err(e) => {
                tracing::error!("Failed to compute is_dm for room {}: {e}", room_id);
                false
            }
        };

        if is_dm {
            dm_rooms.insert(room.room_id().to_owned(), room.clone());
            continue;
        }

        let has_children = !parent_to_children
            .get(&room_id)
            .cloned()
            .unwrap_or_default()
            .is_empty();
        let has_parents = !child_to_parents
            .get(&room_id)
            .cloned()
            .unwrap_or_default()
            .is_empty();

        if has_children && !has_parents {
            server_rooms.insert(room_id, room.clone());
            continue;
        }

        single_rooms.insert(room_id, room.clone());
    }

    state.set_dm_rooms(dm_rooms);
    state.set_server_rooms(server_rooms);
    state.set_single_rooms(single_rooms);
}
