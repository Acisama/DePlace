use std::collections::{HashMap, HashSet};

use matrix_sdk::{
    Client, Room,
    room::RoomMember,
    ruma::{OwnedDeviceId, OwnedRoomId, OwnedUserId},
};
use tokio::sync::watch::{self, Sender};

use crate::matrix_api::{
    account_data::{BreadcrumbsContent, ServerOrderContent, get_account_data},
    sync::{ParentToChildren, reclassify_rooms},
    timeline::TimelineManager,
};

#[derive(Clone)]
pub struct UserDevice {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
}

pub type RoomMap = HashMap<OwnedRoomId, Room>;
pub type MembershipMap = HashMap<OwnedRoomId, HashMap<OwnedUserId, RoomMember>>;

#[derive(Clone)]
pub struct AppState {
    pub client: Client,
    pub user_device: UserDevice,
    dm_rooms: Sender<RoomMap>,
    single_rooms: Sender<RoomMap>,
    server_rooms: Sender<RoomMap>,
    parent_to_children: Sender<ParentToChildren>,
    active_room: Sender<Option<Room>>,
    active_server: Sender<Option<Room>>,
    membership_map: Sender<MembershipMap>,
    pub server_order: Vec<OwnedRoomId>,
    pub breadcrumbs: BreadcrumbsContent,

    pub timeline_manager: TimelineManager,
}

impl AppState {
    pub async fn new(client: Client, user_device: UserDevice) -> Self {
        let breadcrumbs = get_account_data::<BreadcrumbsContent>(&client).await;

        let last_room_id = breadcrumbs.recent_rooms.first().cloned();

        let response = reclassify_rooms(&client).await;

        let last_server = if let Some(room_id) = &last_room_id {
            let parent_ids: HashSet<OwnedRoomId> = response
                .child_to_parents
                .get(room_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|r| r.room_id().to_owned())
                .collect();

            response
                .server_rooms
                .clone()
                .keys()
                .find(|id| parent_ids.contains(*id))
                .cloned()
        } else {
            None
        };

        let (dm_rooms, _) = watch::channel(response.dm_rooms);
        let (server_rooms, _) = watch::channel(response.server_rooms);
        let (single_rooms, _) = watch::channel(response.single_rooms);
        let (parent_to_children, _) = watch::channel(response.parent_to_children);
        let (active_room, _) = watch::channel(last_room_id.and_then(|id| client.get_room(&id)));
        let (active_server, _) = watch::channel(last_server.and_then(|id| client.get_room(&id)));
        let (membership_map, _) = watch::channel(MembershipMap::default());

        let server_order = get_account_data::<ServerOrderContent>(&client).await;

        Self {
            client,
            user_device,
            dm_rooms,
            single_rooms,
            server_rooms,
            parent_to_children,
            active_room,
            active_server,
            membership_map,
            server_order: server_order.servers,
            breadcrumbs,

            timeline_manager: TimelineManager::default(),
        }
    }

    pub fn dm_rooms(&self) -> watch::Receiver<RoomMap> {
        self.dm_rooms.subscribe()
    }

    pub fn server_rooms(&self) -> watch::Receiver<RoomMap> {
        self.server_rooms.subscribe()
    }

    pub fn parent_to_children(&self) -> watch::Receiver<ParentToChildren> {
        self.parent_to_children.subscribe()
    }

    pub fn single_rooms(&self) -> watch::Receiver<RoomMap> {
        self.single_rooms.subscribe()
    }

    pub fn active_room(&self) -> watch::Receiver<Option<Room>> {
        self.active_room.subscribe()
    }

    pub fn active_server(&self) -> watch::Receiver<Option<Room>> {
        self.active_server.subscribe()
    }

    pub fn membership_map(&self) -> watch::Receiver<MembershipMap> {
        self.membership_map.subscribe()
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

    pub(crate) fn set_membership_map(&self, membership_map: MembershipMap) {
        self.membership_map.send_if_modified(|cur| {
            *cur = membership_map;
            true
        });
    }

    pub(crate) fn add_membership(&self, room_id: OwnedRoomId, member: RoomMember) {
        self.membership_map.send_if_modified(|cur| {
            cur.entry(room_id)
                .or_default()
                .insert(member.user_id().to_owned(), member);
            true
        });
    }

    pub(crate) fn set_parent_to_children(&self, parent_to_children: ParentToChildren) {
        self.parent_to_children.send_if_modified(|cur| {
            *cur = parent_to_children;
            true
        });
    }

    pub fn set_active_server(&self, server: Option<Room>) {
        self.active_server.send_if_modified(|cur| {
            let changed = cur.as_ref().map(|r| r.room_id()) != server.as_ref().map(|r| r.room_id());
            if changed {
                *cur = server;
            }
            changed
        });
    }

    pub fn set_active_room(&self, room: Option<Room>) {
        self.active_room.send_if_modified(|cur| {
            let changed = cur.as_ref().map(|r| r.room_id()) != room.as_ref().map(|r| r.room_id());
            if changed {
                *cur = room;
            }
            changed
        });
    }

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
