use std::{cmp::Reverse, collections::HashMap, sync::Arc, time::Instant};

use dashmap::{DashMap, mapref::entry::Entry};
use futures::{StreamExt, future::join_all};
use matrix_sdk::{Client, Room, RoomState, room::ParentSpace};
use ruma::{
    MilliSecondsSinceUnixEpoch, OwnedRoomId, RoomId, events::space::child::SpaceChildEventContent,
};
use tokio::sync::watch;

use crate::{
    Shared, matrix_api::account_data::ServerOrderContent, member::ProfileStore, state::UserDevice,
};

use super::{
    DePlaceRoom,
    hierarchy::{ChildOrder, SpaceHierarchy},
};

#[cfg(feature = "iced")]
use crate::Avatar;

pub type RoomMap = DashMap<OwnedRoomId, DePlaceRoom>;

pub struct RoomStore {
    client: Client,
    handle: tokio::runtime::Handle,
    profile_store: Arc<ProfileStore>,
    own_device: Arc<UserDevice>,

    room_map: RoomMap,

    dms: Arc<Shared<Vec<DePlaceRoom>>>,
    hierarchy: Arc<Shared<SpaceHierarchy>>,
    pub(crate) changed: watch::Sender<()>,
}

impl RoomStore {
    pub async fn new(
        client: &Client,
        profile_store: Arc<ProfileStore>,
        last_server_order: ServerOrderContent,
        own_device: Arc<UserDevice>,
    ) -> Self {
        let rooms = client.rooms();

        let room_map = rooms
            .iter()
            .map(|r| {
                (
                    r.room_id().to_owned(),
                    DePlaceRoom::new(r, &profile_store, own_device.clone()),
                )
            })
            .collect();

        let dm_rooms: Vec<Room> = rooms.iter().filter(|r| r.is_dm()).cloned().collect();

        let store = Self {
            client: client.clone(),
            handle: tokio::runtime::Handle::current(),
            profile_store,
            room_map,
            own_device,
            dms: Arc::new(Shared::new(Vec::new())),
            hierarchy: Arc::new(Shared::new(SpaceHierarchy::new(last_server_order))),
            changed: watch::channel(()).0,
        };
        store.load_hierarchy(rooms).await;

        // Awaited so DMs have their other member (name, avatar) ready before
        // the UI ever renders, instead of waiting on the slower full-room
        // background load in `spawn_initial_load`.
        join_all(
            dm_rooms
                .iter()
                .map(|room| store.profile_store.load_room_members(room)),
        )
        .await;

        store.recompute_dms();

        #[cfg(feature = "iced")]
        for room in store.room_map.iter() {
            store.spawn_avatar_load(room.value().clone());
        }

        store
    }

    /// Builds the full parent/child hierarchy for every room the client
    /// currently knows about, concurrently. Meant to be awaited once, right
    /// after construction and before the app is shown, so the hierarchy is
    /// already populated instead of trickling in room-by-room as `get_room`
    /// is called.
    async fn load_hierarchy(&self, rooms: Vec<Room>) {
        let room_count = rooms.len();
        let start = Instant::now();

        join_all(
            rooms
                .iter()
                .map(|room| apply_parent_update(&self.hierarchy, room)),
        )
        .await;

        tracing::debug!(
            "Loaded hierarchy for {room_count} rooms in {:?}",
            start.elapsed()
        );
    }

    #[cfg(feature = "iced")]
    fn spawn_room_load(&self, dp_room: DePlaceRoom) {
        let hierarchy = self.hierarchy.clone();
        let room = dp_room.room.clone();

        self.spawn_avatar_load(dp_room);

        self.handle.spawn(async move {
            apply_parent_update(&hierarchy, &room).await;
        });
    }

    #[cfg(feature = "iced")]
    pub(crate) fn spawn_avatar_load(&self, dp_room: DePlaceRoom) {
        let changed = self.changed.clone();
        let changed_clone = changed.clone();
        let client = self.client.clone();
        let client_clone = client.clone();
        let url = dp_room.info.data.load().avatar_url();
        let url_clone = url.clone();

        let cell = dp_room.info.clone();
        let cell_clone = cell.clone();

        self.handle.spawn(async move {
            let avatar = Avatar::load_avatar(&client, url, false).await;
            cell.change(|info| {
                info.avatar_handle = avatar.clone();
            });
            changed.send_modify(|_| ());
        });
        self.handle.spawn(async move {
            let small_avatar = Avatar::load_avatar(&client_clone, url_clone, true).await;
            cell_clone.change(|info| {
                info.small_avatar_handle = small_avatar.clone();
            });
            changed_clone.send_modify(|_| ());
        });
    }

    /// Re-fetches `room_id`'s parent spaces and applies them to the
    /// hierarchy. Called on initial room load, and should also be called
    /// whenever an `m.space.child`/`m.space.parent` state event comes in for
    /// a room, since the hierarchy isn't kept live by anything else.
    pub(crate) async fn refresh_hierarchy(&self, room_id: &RoomId) {
        let Some(room) = self.client.get_room(room_id) else {
            return;
        };

        apply_parent_update(&self.hierarchy, &room).await;
    }

    pub fn get_room(&self, room_id: &RoomId) -> Option<DePlaceRoom> {
        self.get_room_with_if_created(room_id).map(|(room, _)| room)
    }

    pub(crate) fn get_room_with_if_created(&self, room_id: &RoomId) -> Option<(DePlaceRoom, bool)> {
        if let Some(room) = self.room_map.get(room_id) {
            return Some((room.clone(), false));
        }

        let sdk_room = self.client.get_room(room_id)?;

        let (room, created) = match self.room_map.entry(room_id.to_owned()) {
            Entry::Occupied(e) => (e.get().clone(), false),
            Entry::Vacant(e) => {
                let room =
                    DePlaceRoom::new(&sdk_room, &self.profile_store, self.own_device.clone());
                e.insert(room.clone());
                (room, true)
            }
        };

        #[cfg(feature = "iced")]
        if created {
            self.spawn_room_load(room.clone());
        }

        Some((room, created))
    }

    pub fn get_all_rooms(&self) -> Vec<DePlaceRoom> {
        let mut rooms: Vec<DePlaceRoom> = self.room_map.iter().map(|e| e.value().clone()).collect();
        rooms.sort_by(|a, b| a.room_id().cmp(b.room_id()));
        rooms
    }

    pub fn dm_rooms(&self) -> Arc<Vec<DePlaceRoom>> {
        self.dms.get()
    }

    /// Re-scans every room the client knows about and rebuilds the cached DM
    /// list. Also doubles as the seeding mechanism now that `RoomListService`
    /// no longer drives room discovery: `get_room` inserts (and spawns a load
    /// for) any room that isn't in the map yet.
    pub fn recompute_dms(&self) {
        let mut dms: Vec<(DePlaceRoom, Option<MilliSecondsSinceUnixEpoch>)> = self
            .client
            .rooms()
            .into_iter()
            .filter(|room| matches!(room.state(), RoomState::Joined | RoomState::Invited))
            .filter_map(|room| {
                let is_dm = room.is_dm();
                let dp_room = self.get_room(room.room_id())?;

                if !is_dm {
                    return None;
                }

                Some((dp_room, room.latest_event_timestamp()))
            })
            .collect();
        dms.sort_by_key(|(_, timestamp)| Reverse(*timestamp));

        self.dms
            .set(dms.into_iter().map(|(room, _)| room).collect());
    }

    pub fn servers(&self) -> Arc<Vec<OwnedRoomId>> {
        self.hierarchy.read().servers.clone()
    }

    /// Bumps on every hierarchy mutation: reordering, and any `m.space.child`
    /// structural change (room added/removed/reparented).
    pub fn hierarchy_version(&self) -> u64 {
        self.hierarchy.version()
    }

    /// Bumps whenever the cached DM list is recomputed.
    pub fn dms_version(&self) -> u64 {
        self.dms.version()
    }

    pub fn get_server_of(&self, room_id: &RoomId) -> Option<DePlaceRoom> {
        let server_id = self.hierarchy.read().get_server_of(room_id)?;
        self.get_room(&server_id)
    }

    pub fn get_first_child_of(&self, parent: &RoomId) -> Option<DePlaceRoom> {
        let child_id = self.hierarchy.read().get_first_child_of(parent)?;
        self.get_room(&child_id)
    }

    pub fn get_children(&self, parent: &RoomId) -> Option<Vec<DePlaceRoom>> {
        let children = self.hierarchy.read().get_children(parent);
        if children.is_empty() {
            return None;
        }
        Some(children.iter().filter_map(|id| self.get_room(id)).collect())
    }

    pub fn get_all_children(&self, parent: &RoomId) -> Vec<DePlaceRoom> {
        self.hierarchy
            .read()
            .get_all_children(parent)
            .iter()
            .filter_map(|id| self.get_room(id))
            .collect()
    }

    pub fn reorder_servers(&self, index: usize, target_index: usize) -> ServerOrderContent {
        self.hierarchy.change(|h| {
            h.reorder_servers(index, target_index);
        });

        ServerOrderContent {
            servers: (*self.hierarchy.read().servers).clone(),
        }
    }
}

async fn apply_parent_update(hierarchy: &Shared<SpaceHierarchy>, room: &Room) {
    let room_id = room.room_id().to_owned();
    let parents = fetch_parent_spaces(room, &room_id).await;

    hierarchy.change(move |h| {
        h.update_child_parents(room_id.clone(), parents.clone());
    });
}

async fn fetch_parent_spaces(room: &Room, room_id: &RoomId) -> HashMap<OwnedRoomId, ChildOrder> {
    let stream = match room.parent_spaces().await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::error!("Failed to get parent spaces for {room_id}: {e}");
            return HashMap::new();
        }
    };

    let mut parents = HashMap::new();

    for res in stream.collect::<Vec<_>>().await {
        let parent_room = match res {
            Ok(ParentSpace::Reciprocal(room)) => room,
            Err(e) => {
                tracing::error!("Failed to get parent space for {room_id}: {e}");
                continue;
            }
            _ => continue,
        };

        let original_event = match parent_room
            .get_state_event_static_for_key::<SpaceChildEventContent, _>(room_id)
            .await
        {
            Ok(child) => child.and_then(|raw| match raw.deserialize() {
                Ok(child) => child.as_sync().cloned(),
                Err(e) => {
                    tracing::error!("Failed to deserialize child event: {e}");
                    None
                }
            }),
            Err(e) => {
                tracing::error!("Failed to get child event for {room_id}: {e}");
                None
            }
        }
        .and_then(|v| v.as_original().cloned());

        let child_order = ChildOrder {
            order: original_event
                .as_ref()
                .and_then(|v| v.content.order.clone())
                .map(|o| o.to_string()),
            origin_server_ts: original_event.map(|v| v.origin_server_ts),
        };

        parents.insert(parent_room.room_id().to_owned(), child_order);
    }

    parents
}
