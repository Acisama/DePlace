use std::collections::HashMap;

use matrix_sdk::{
    Client, Room,
    ruma::{OwnedDeviceId, OwnedRoomId, OwnedUserId},
};
use tokio::sync::watch::{self, Sender};

#[derive(Clone)]
pub struct UserDevice {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
}

type RoomMap = HashMap<OwnedRoomId, Room>;

#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub user_device: UserDevice,
    dm_rooms: Sender<RoomMap>,
    single_rooms: Sender<RoomMap>,
    server_rooms: Sender<RoomMap>,
    active_room: Sender<Option<Room>>,
}

impl AppState {
    pub async fn new(client: Client, user_device: UserDevice) -> Self {
        let (dm_rooms, _) = watch::channel(HashMap::new());
        let (single_rooms, _) = watch::channel(HashMap::new());
        let (server_rooms, _) = watch::channel(HashMap::new());
        let (active_room, _) = watch::channel(None);

        Self {
            client,
            user_device,
            dm_rooms,
            single_rooms,
            server_rooms,
            active_room,
        }
    }

    pub fn dm_rooms(&self) -> watch::Receiver<RoomMap> {
        self.dm_rooms.subscribe()
    }

    pub fn server_rooms(&self) -> watch::Receiver<RoomMap> {
        self.server_rooms.subscribe()
    }

    pub fn single_rooms(&self) -> watch::Receiver<RoomMap> {
        self.single_rooms.subscribe()
    }

    pub fn active_room(&self) -> watch::Receiver<Option<Room>> {
        self.active_room.subscribe()
    }

    pub(crate) fn set_dm_rooms(&self, rooms: RoomMap) {
        Self::send_if_keys_changed(&self.dm_rooms, rooms);
    }

    pub(crate) fn set_server_rooms(&self, rooms: RoomMap) {
        Self::send_if_keys_changed(&self.server_rooms, rooms);
    }

    pub(crate) fn set_single_rooms(&self, rooms: RoomMap) {
        Self::send_if_keys_changed(&self.single_rooms, rooms);
    }

    /// Called from the frontend when the user selects a different room.
    pub fn set_active_room(&self, room: Option<Room>) {
        self.active_room.send_if_modified(|cur| {
            let changed = cur.as_ref().map(|r| r.room_id()) != room.as_ref().map(|r| r.room_id());
            if changed {
                *cur = room;
            }
            changed
        });
    }

    /// `Room` has no `PartialEq`, and `HashMap` iteration order isn't stable
    /// across two independently-built maps, so equality is judged by set
    /// membership rather than by comparing entries pairwise.
    fn send_if_keys_changed(sender: &Sender<RoomMap>, rooms: RoomMap) {
        sender.send_if_modified(|cur| {
            let changed = cur.len() != rooms.len() || rooms.keys().any(|id| !cur.contains_key(id));
            if changed {
                *cur = rooms;
            }
            changed
        });
    }
}
