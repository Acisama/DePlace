use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use matrix_sdk::{
    Client, Room,
    room::RoomMember,
    ruma::{OwnedDeviceId, OwnedRoomId, OwnedUserId},
};
use ruma::events::presence::PresenceEventContent;
use tokio::sync::watch::{self, Ref, Sender};

use crate::{
    matrix_api::{
        account_data::{
            BreadcrumbsContent, ServerOrderContent, get_account_data, set_account_data,
        },
        sync::{ParentToChildren, ParentToChildrenOrderStr, reclassify_rooms},
    },
    settings::Settings,
};

#[derive(Clone, Debug)]
pub struct UserDevice {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
}

pub type RoomMap = HashMap<OwnedRoomId, Room>;
pub type MembershipMap = HashMap<OwnedRoomId, HashMap<OwnedUserId, RoomMember>>;
pub type PresenceMap = HashMap<OwnedUserId, PresenceEventContent>;

/// Cheaply clonable AppState since the data is all
/// wrapped in an `Arc`. Access only over functions,
/// no direct field access.
#[derive(Clone, Debug)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

#[derive(Debug)]
struct AppStateInner {
    pub client: Client,
    pub user_device: UserDevice,
    pub settings: Settings,

    dm_rooms: Sender<RoomMap>,
    single_rooms: Sender<RoomMap>,
    server_rooms: Sender<RoomMap>,
    /// Monotonically increasing room version counter, gets increased if anything notable changes in any room
    pub room_version: Sender<u64>,

    parent_to_children: Sender<ParentToChildrenOrderStr>,
    parent_to_all_children: Sender<ParentToChildren>,

    active_room: Sender<Option<Room>>,
    active_server: Sender<Option<Room>>,
    membership_map: Sender<MembershipMap>,
    presence_map: Sender<PresenceMap>,

    server_order: Sender<Vec<OwnedRoomId>>,

    /// Last accessed servers and rooms, since the data is only
    /// needed as snapshots, it is held in a mutex
    breadcrumbs: Mutex<BreadcrumbsContent>,
}

impl AppState {
    pub async fn new(client: Client, user_device: UserDevice, settings: Settings) -> Self {
        let breadcrumbs_content = get_account_data::<BreadcrumbsContent>(&client).await;

        let last_room_id = breadcrumbs_content.recent_rooms.first().cloned();

        let breadcrumbs = Mutex::new(breadcrumbs_content);

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
        let (parent_to_all_children, _) = watch::channel(response.parent_to_all_children);

        let (active_room, _) = watch::channel(last_room_id.and_then(|id| client.get_room(&id)));
        let (active_server, _) = watch::channel(last_server.and_then(|id| client.get_room(&id)));

        let (membership_map, _) = watch::channel(MembershipMap::default());
        let (presence_map, _) = watch::channel(PresenceMap::default());

        let server_order_data = get_account_data::<ServerOrderContent>(&client).await;
        let (server_order, _) = watch::channel(server_order_data.servers);

        let (room_version, _) = watch::channel(0);

        Self {
            inner: Arc::new(AppStateInner {
                client,
                user_device,
                settings,

                dm_rooms,
                single_rooms,
                server_rooms,
                room_version,

                parent_to_children,
                parent_to_all_children,

                active_room,
                active_server,

                membership_map,
                presence_map,

                server_order,
                breadcrumbs,
            }),
        }
    }

    pub fn room_version(&self) -> Ref<'_, u64> {
        self.inner.room_version.borrow()
    }

    pub fn bump_room_version(&self) {
        self.inner.room_version.send_modify(|v| *v += 1);
    }

    /// Retrieves the `matrix-sdk::Client` of the app
    pub fn client(&self) -> Client {
        self.inner.client.clone()
    }

    /// Retrieves the `Settings` of the app
    pub fn settings(&self) -> Settings {
        self.inner.settings.clone()
    }

    pub fn user_device(&self) -> &UserDevice {
        &self.inner.user_device
    }

    // Getters

    pub fn dm_rooms(&self) -> watch::Receiver<RoomMap> {
        self.inner.dm_rooms.subscribe()
    }

    pub fn server_rooms(&self) -> watch::Receiver<RoomMap> {
        self.inner.server_rooms.subscribe()
    }

    pub fn parent_to_children(&self) -> watch::Receiver<ParentToChildrenOrderStr> {
        self.inner.parent_to_children.subscribe()
    }

    pub fn single_rooms(&self) -> watch::Receiver<RoomMap> {
        self.inner.single_rooms.subscribe()
    }

    pub fn active_room(&self) -> watch::Receiver<Option<Room>> {
        self.inner.active_room.subscribe()
    }

    pub fn active_server(&self) -> watch::Receiver<Option<Room>> {
        self.inner.active_server.subscribe()
    }

    pub fn membership_map(&self) -> watch::Receiver<MembershipMap> {
        self.inner.membership_map.subscribe()
    }

    pub fn presence_map(&self) -> watch::Receiver<PresenceMap> {
        self.inner.presence_map.subscribe()
    }

    pub fn server_order(&self) -> watch::Receiver<Vec<OwnedRoomId>> {
        self.inner.server_order.subscribe()
    }

    /// Returns a snapshot copy of the breadcrumbs
    pub fn breadcrumbs(&self) -> BreadcrumbsContent {
        self.inner
            .breadcrumbs
            .lock()
            .expect("breadcrumbs mutex poisoned")
            .clone()
    }

    // Mutators

    pub(crate) fn set_dm_rooms(&self, rooms: RoomMap) {
        Self::send_if_keys_changed(&self.inner.dm_rooms, rooms);
    }

    pub(crate) fn set_server_rooms(&self, rooms: RoomMap) {
        Self::send_if_keys_changed(&self.inner.server_rooms, rooms);
    }

    pub(crate) fn set_single_rooms(&self, rooms: RoomMap) {
        Self::send_if_keys_changed(&self.inner.single_rooms, rooms);
    }

    pub(crate) fn set_membership_map(&self, membership_map: MembershipMap) {
        self.inner.membership_map.send_if_modified(|cur| {
            *cur = membership_map;
            true
        });
    }

    pub(crate) fn add_membership(&self, room_id: OwnedRoomId, member: RoomMember) {
        self.inner.membership_map.send_if_modified(|cur| {
            cur.entry(room_id)
                .or_default()
                .insert(member.user_id().to_owned(), member);
            true
        });
    }

    pub(crate) fn add_presences(&self, presences: PresenceMap) {
        self.inner.presence_map.send_if_modified(|cur| {
            cur.extend(presences);
            true
        });
    }

    pub(crate) fn set_parent_to_children(&self, parent_to_children: ParentToChildrenOrderStr) {
        self.inner.parent_to_children.send_if_modified(|cur| {
            *cur = parent_to_children;
            true
        });
    }

    /// Mutates the breadcrumbs in-place using a closure. This function is private since
    /// breadcrumbs should not be updated manually.
    fn update_breadcrumbs<F>(&self, f: F)
    where
        F: FnOnce(&mut BreadcrumbsContent),
    {
        let mut guard = self
            .inner
            .breadcrumbs
            .lock()
            .expect("breadcrumbs mutex poisoned");
        f(&mut guard);
    }

    /// Sets the active server and the new active room. Calling `set_active_room` after
    /// this is redundant.
    pub fn set_active_server(&self, server: Option<Room>) {
        let mut server_changed = false;

        // change the server
        self.inner.active_server.send_if_modified(|cur| {
            let changed = cur.as_ref().map(|r| r.room_id()) != server.as_ref().map(|r| r.room_id());
            if changed {
                *cur = server.clone();
                server_changed = true;
            }
            changed
        });

        // if the server was changed, also change the room
        if server_changed {
            let server_id = server.as_ref().map(|r| r.room_id());
            let breadcrumbs = self.breadcrumbs();

            let new_room_id = if let Some(server_id) = server_id {
                breadcrumbs
                    .last_space_ids
                    .get(server_id)
                    .cloned()
                    .or_else(|| {
                        let mut children: Vec<(Room, Option<String>)> = self
                            .inner
                            .parent_to_children
                            .borrow()
                            .get(server_id)
                            .cloned()
                            .unwrap_or_default()
                            .values()
                            .cloned()
                            .collect();

                        children.sort_by_key(|(r, o)| {
                            o.clone().unwrap_or_else(|| r.room_id().to_string())
                        });
                        children.first().map(|(r, _)| r.room_id().to_owned())
                    })
            } else {
                breadcrumbs.last_dm_id.clone().or_else(|| {
                    self.inner
                        .dm_rooms
                        .borrow()
                        .values()
                        .next()
                        .map(|r| r.room_id().to_owned())
                })
            };

            let new_room = new_room_id.and_then(|id| self.client().get_room(&id));
            self.set_active_room(new_room);
        }
    }

    /// Set the currently focused room. This function als takes care of updating
    /// the breadcrumbs and active server
    pub fn set_active_room(&self, room: Option<Room>) {
        let mut room_changed = false;

        self.inner.active_room.send_if_modified(|cur| {
            let changed = cur.as_ref().map(|r| r.room_id()) != room.as_ref().map(|r| r.room_id());
            if changed {
                *cur = room.clone();
                room_changed = true;
            }
            changed
        });

        if let Some(room) = room
            && room_changed
        {
            let room_id = room.room_id().to_owned();
            let active_server_id = self
                .inner
                .parent_to_all_children
                .borrow()
                .clone()
                .into_iter()
                .find(|(_, v)| v.contains_key(&room_id))
                .map(|(k, _)| k);

            let active_server = active_server_id
                .and_then(|id| self.server_rooms().borrow().clone().get(&id).cloned());

            if self
                .active_server()
                .borrow()
                .clone()
                .map(|s| s.room_id().to_owned())
                != active_server.as_ref().map(|s| s.room_id().to_owned())
            {
                self.set_active_server(active_server.clone());
            }

            self.update_breadcrumbs(|breadcrumbs| {
                // Remove duplicates
                breadcrumbs.recent_rooms.retain(|id| id != &room_id);
                // Insert visited room at top
                breadcrumbs.recent_rooms.insert(0, room_id.clone());
                // Truncate to 25
                breadcrumbs.recent_rooms.truncate(25);

                if let Some(active_server) = active_server {
                    breadcrumbs
                        .last_space_ids
                        .insert(active_server.room_id().to_owned(), room_id);
                } else {
                    breadcrumbs.last_dm_id = Some(room_id);
                    breadcrumbs.dms_last = true;
                }
            });

            let client = self.client();
            let breadcrumbs = self.breadcrumbs();
            tokio::spawn(async move { set_account_data(&client, breadcrumbs).await });
        }
    }

    pub fn set_server_order(&self, server_order: Vec<OwnedRoomId>) {
        self.inner.server_order.send_if_modified(|cur| {
            if *cur != server_order {
                *cur = server_order;
                true
            } else {
                false
            }
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
