use std::pin::pin;
use std::time::Duration;

use futures_util::StreamExt;
use matrix_sdk::{Client, config::SyncSettings, ruma::presence::PresenceState, sync::SyncResponse};

use crate::{
    matrix_api::{
        members::run_membership_map_update,
        presence::{get_presences, handle_presences},
        save_session,
    },
    notifications::on_message,
    state::AppState,
};

pub fn spawn_room_sync(client: &Client, state: &AppState) {
    if let Err(e) = client.event_cache().subscribe() {
        tracing::error!("Failed to subscribe to event cache: {e}");
    }

    tokio::spawn(run_sync_stream(client.clone(), state.clone()));
    tokio::spawn(run_keystore_save_stream(client.clone()));
    tokio::spawn(run_membership_map_update(client.clone(), state.clone()));
    tokio::spawn(get_presences(client.clone(), state.clone()));

    client.add_event_handler_context(state.clone());
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
        .timeout(Duration::from_secs(30));

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

    handle_presences(&result.presence, state);

    state.bump_sync_tick();
}

// pub struct ClasifiedRooms {
//     pub dm_rooms: DmRoomMap,
//     pub single_rooms: RoomMap,
//     pub server_rooms: RoomMap,

//     pub parent_to_children: ParentToChildrenOrderStr,
//     pub parent_to_all_children: ParentToChildren,

//     pub child_to_parents: ChildToParents,
// }

// pub async fn reclassify_rooms(client: &Client) -> ClasifiedRooms {
//     let mut dm_rooms = IndexMap::new();
//     let mut server_rooms = HashMap::new();
//     let mut single_rooms = HashMap::new();

//     let mut parent_to_children: ParentToChildrenOrderStr = HashMap::new();
//     let mut parent_to_all_children: ParentToChildren = HashMap::new();

//     let mut child_to_parents: ChildToParents = HashMap::new();

//     let rooms = client.rooms();
//     for room in rooms {
//         let parents = match room.parent_spaces().await {
//             Ok(parents) => parents,
//             Err(e) => {
//                 tracing::error!(
//                     "Failed to get parent spaces for room {}: {e}",
//                     room.room_id()
//                 );
//                 continue;
//             }
//         };

//         let parents_res = parents.collect::<Vec<_>>().await;

//         let parents: Vec<Room> = parents_res
//             .iter()
//             .filter_map(|res| {
//                 if let Ok(ParentSpace::Reciprocal(room)) = res {
//                     Some(room.clone())
//                 } else {
//                     None
//                 }
//             })
//             .collect();

//         child_to_parents.insert(room.room_id().to_owned(), parents.clone());

//         let room_id = room.room_id();

//         for parent in parents {
//             let order = parent
//                 .get_state_event_static_for_key::<SpaceChildEventContent, _>(room_id)
//                 .await
//                 .map_err(|e| {
//                     tracing::error!("Failed to get state event for key {}: {e}", room_id);
//                     e
//                 })
//                 .ok()
//                 .flatten()
//                 .and_then(|raw| {
//                     raw.deserialize()
//                         .map_err(|e| tracing::error!("Failed to deserialize state event: {e}"))
//                         .ok()
//                 })
//                 .and_then(|v| v.as_sync().cloned())
//                 .and_then(|v| v.as_original().cloned())
//                 .and_then(|v| v.content.order.clone())
//                 .map(|o| o.to_string());

//             let entry = parent_to_children
//                 .entry(parent.room_id().to_owned())
//                 .or_default();
//             entry.insert(room_id.to_owned(), (room.clone(), order));
//         }
//     }

//     for room in client.rooms() {
//         let room_id = room.room_id().to_owned();

//         let is_dm = match room.compute_is_dm().await {
//             Ok(is_dm) => is_dm,
//             Err(e) => {
//                 tracing::error!("Failed to compute is_dm for room {}: {e}", room_id);
//                 false
//             }
//         };

//         if is_dm {
//             dm_rooms.insert(room.room_id().to_owned(), room.clone());
//             continue;
//         }

//         let has_children = !parent_to_children
//             .get(&room_id)
//             .cloned()
//             .unwrap_or_default()
//             .is_empty();
//         let has_parents = !child_to_parents
//             .get(&room_id)
//             .cloned()
//             .unwrap_or_default()
//             .is_empty();

//         if has_children && !has_parents {
//             server_rooms.insert(room_id, room.clone());
//             continue;
//         }

//         single_rooms.insert(room_id, room.clone());
//     }

//     for (parent_id, children) in &parent_to_children {
//         for (child_id, (child, _)) in children {
//             parent_to_all_children
//                 .entry(parent_id.clone())
//                 .or_default()
//                 .insert(child_id.clone(), child.clone());
//         }
//     }

//     let latest_events = client.latest_events().await;

//     for id in dm_rooms.keys() {
//         if let Err(e) = latest_events.listen_to_room(id).await {
//             tracing::error!("Failed to listen to room {}: {e}", id);
//         }
//     }

//     dm_rooms.sort_by_key(|_, r| Reverse(r.latest_event_timestamp()));

//     ClasifiedRooms {
//         dm_rooms,
//         server_rooms,
//         single_rooms,

//         parent_to_children,
//         parent_to_all_children,

//         child_to_parents,
//     }
// }
