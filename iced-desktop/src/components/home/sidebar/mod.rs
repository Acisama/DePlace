use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashSet},
    time::{Duration, Instant},
};

use channels::render_channels;
use deplace_core::{
    RoomMap,
    matrix_api::sync::ParentToChildrenOrderStr,
    state::{ActiveServer, ActiveServerId, to_stream},
};
use matrix_sdk::ruma::{OwnedMxcUri, OwnedRoomId};
use server_column::render_server_column;
use tokio::sync::watch::Receiver;

use crate::common::*;

mod channels;
mod pill;
mod server_column;

#[derive(Clone, Debug)]
pub enum SidebarMessage {
    ChangeActiveRoom(Option<Room>),
    ChangeActiveServer(ActiveServer),
    ActiveServerChange(ActiveServer),
    NeedAvatar(OwnedMxcUri),
    ServerHovered(ActiveServerId),
    ServerHoverEnded(ActiveServerId),
}

impl NeedsAvatarExt for SidebarMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        SidebarMessage::NeedAvatar(uri)
    }
}

pub enum SidebarAction {
    None,
    Run(Task<()>),
}

#[derive(Clone)]
pub struct Sidebar {
    state: AppState,

    server_rooms: Receiver<RoomMap>,
    server_order: Receiver<Vec<OwnedRoomId>>,
    dm_rooms: Receiver<RoomMap>,

    parent_to_children: Receiver<ParentToChildrenOrderStr>,

    active_room: Receiver<Option<Room>>,
    active_server: Receiver<ActiveServer>,

    hovered_server: Option<ActiveServerId>,

    /// Used to check if any avatars the sidebar depends on changed
    avatar_states_for_hash: BTreeSet<OwnedMxcUri>,
}

impl Hash for Sidebar {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.state.room_version().hash(state);
        self.state.active_room_version().hash(state);
        self.state.active_server_version().hash(state);
        self.hovered_server.hash(state);

        for uri in &self.avatar_states_for_hash {
            self.state
                .avatar_cache()
                .get(uri)
                .unwrap_or_default()
                .hash(state)
        }
    }
}

impl Sidebar {
    pub fn new(state: AppState) -> Self {
        Self {
            server_rooms: state.server_rooms(),
            server_order: state.server_order(),
            dm_rooms: state.dm_rooms(),

            parent_to_children: state.parent_to_children(),

            active_room: state.active_room(),
            active_server: state.active_server(),

            hovered_server: None,

            state,
            avatar_states_for_hash: BTreeSet::new(),
        }
    }

    pub fn subscription(&self) -> Subscription<SidebarMessage> {
        Subscription::run_with(self.state.clone(), |state| {
            to_stream(state.active_server(), |server| {
                SidebarMessage::ActiveServerChange(server)
            })
        })
    }

    pub fn update(&mut self, message: SidebarMessage) -> SidebarAction {
        match message {
            SidebarMessage::ChangeActiveRoom(room) => {
                let state = self.state.clone();
                SidebarAction::Run(Task::future(async move {
                    state
                        .set_active_server(state.set_active_room(room, false).await, false)
                        .await;
                }))
            }
            SidebarMessage::ChangeActiveServer(server) => {
                let state = self.state.clone();
                SidebarAction::Run(Task::future(async move {
                    state.set_active_server(server, true).await;
                }))
            }
            SidebarMessage::NeedAvatar(uri) => {
                self.avatar_states_for_hash.retain(|u| {
                    !matches!(
                        self.state.avatar_cache().get(u).unwrap_or_default(),
                        MediaState::Failed | MediaState::Loaded(_)
                    )
                });
                self.avatar_states_for_hash.insert(uri.clone());

                let avatar_cache = self.state.avatar_cache().clone();
                SidebarAction::Run(Task::future(
                    async move { avatar_cache.load_avatar(&uri).await },
                ))
            }
            SidebarMessage::ServerHovered(server) => {
                self.hovered_server = Some(server);
                SidebarAction::None
            }
            SidebarMessage::ServerHoverEnded(server) => {
                if self.hovered_server == Some(server) {
                    self.hovered_server = None;
                }
                SidebarAction::None
            }
            SidebarMessage::ActiveServerChange(new) => SidebarAction::None,
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, SidebarMessage> {
        let active_room = self.active_room.borrow().clone();
        let active_server = self.active_server.borrow().clone();

        let rooms_map = self.server_rooms.borrow().clone();
        let ordered_server_ids_vec = self.server_order.clone();

        let ordered_server_ids: HashSet<OwnedRoomId> =
            ordered_server_ids_vec.borrow().iter().cloned().collect();
        let all_server_ids: HashSet<OwnedRoomId> = rooms_map.keys().cloned().collect();

        let unsorted_server_ids: Vec<OwnedRoomId> = all_server_ids
            .difference(&ordered_server_ids)
            .cloned()
            .collect();

        let mut sorted_rooms: Vec<Room> = self
            .server_order
            .borrow()
            .iter()
            .filter_map(|id| rooms_map.get(id).cloned())
            .collect();

        let mut unsorted_rooms: Vec<Room> = unsorted_server_ids
            .iter()
            .filter_map(|id| rooms_map.get(id).cloned())
            .collect();
        unsorted_rooms.sort_by_key(|r| r.room_id().to_string());
        sorted_rooms.extend(unsorted_rooms);

        let channels = match &active_server {
            ActiveServer::Dms => {
                let mut rooms: Vec<Room> = self.dm_rooms.borrow().values().cloned().collect();
                rooms.sort_by_key(|r| Reverse(r.latest_event_timestamp()));
                rooms
            }
            ActiveServer::Server(server) => {
                let mut children: Vec<(Room, Option<String>)> = self
                    .parent_to_children
                    .borrow()
                    .get(server.room_id())
                    .cloned()
                    .unwrap_or_default()
                    .values()
                    .cloned()
                    .collect();

                children.sort_by(|(r1, o1), (r2, o2)| {
                    let k1 = o1.as_deref().unwrap_or_else(|| r1.room_id().as_str());
                    let k2 = o2.as_deref().unwrap_or_else(|| r2.room_id().as_str());
                    k1.cmp(k2)
                });

                children.into_iter().map(|(room, _)| room).collect()
            }
        };

        w::row![
            render_server_column(
                theme,
                structure,
                sorted_rooms,
                active_server.clone(),
                &self.hovered_server,
                self.state.avatar_cache()
            ),
            Space::new().width(structure.small_gap),
            // floating_tile(theme, structure, text(active_room.get_name()))
            render_channels(theme, structure, active_server, channels)
        ]
        .into()
    }
}
