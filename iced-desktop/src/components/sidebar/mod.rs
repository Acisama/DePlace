use std::collections::{BTreeSet, HashSet};

use deplace_core::{
    RoomMap,
    state::cache::{AvatarCache, MediaState},
};
use iced::widget::svg;
use matrix_sdk::ruma::{OwnedMxcUri, OwnedRoomId};
use tokio::sync::watch::Receiver;

use crate::common::*;

#[derive(Clone, Debug)]
pub enum SidebarMessage {
    ActiveRoomChange(Option<Room>),
    ActiveServerChange(Option<Room>),
    NeedAvatar(OwnedMxcUri),
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

pub struct Sidebar {
    state: AppState,
    server_rooms: Receiver<RoomMap>,
    server_order: Receiver<Vec<OwnedRoomId>>,

    /// Used to check if any avatars the sidebar depends on changed
    avatar_states_for_hash: BTreeSet<OwnedMxcUri>,
}

impl Hash for Sidebar {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.state.room_version().hash(state);
        for uri in &self.avatar_states_for_hash {
            if let MediaState::Loading = self.state.avatar_cache().get(uri).unwrap_or_default() {
                0.hash(state)
            }
        }
    }
}

impl Sidebar {
    pub fn new(state: AppState) -> Self {
        Self {
            server_rooms: state.server_rooms(),
            server_order: state.server_order(),

            state,
            avatar_states_for_hash: BTreeSet::new(),
        }
    }

    pub fn update(&mut self, message: SidebarMessage) -> SidebarAction {
        match message {
            SidebarMessage::ActiveRoomChange(room) => {
                let state = self.state.clone();
                SidebarAction::Run(Task::future(async move {
                    state
                        .set_active_server(state.set_active_room(room, false).await, false)
                        .await;
                }))
            }
            SidebarMessage::ActiveServerChange(server) => {
                let state = self.state.clone();
                SidebarAction::Run(Task::future(async move {
                    state.set_active_server(server, true).await;
                }))
            }
            SidebarMessage::NeedAvatar(uri) => {
                self.avatar_states_for_hash.retain(|u| {
                    matches!(
                        self.state.avatar_cache().get(u).unwrap_or_default(),
                        MediaState::Loading
                    )
                });
                self.avatar_states_for_hash.insert(uri.clone());

                let avatar_cache = self.state.avatar_cache().clone();
                SidebarAction::Run(Task::future(
                    async move { avatar_cache.load_avatar(&uri).await },
                ))
            }
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, SidebarMessage> {
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

        w::row![
            render_server_column(theme, structure, sorted_rooms, self.state.avatar_cache()),
            Space::new().width(structure.small_gap),
            floating_tile(theme, structure, "test")
        ]
        .into()
    }
}

fn render_server_column(
    theme: Theme,
    structure: Structure,
    sorted_rooms: Vec<Room>,
    avatar_cache: &AvatarCache,
) -> Element<'static, SidebarMessage> {
    let icon_handle = iced::advanced::svg::Handle::from_memory(include_bytes!(
        "../../../../assets/deplace_icon.svg"
    ));
    let icon_size = structure.server_column.icon_size;

    let mut column = w::column![
        svg(icon_handle).width(icon_size).height(icon_size),
        container(
            Space::new()
                .width(icon_size)
                .height(structure.divider_width)
        )
        .style(move |_| w::container::Style {
            background: Some(theme.border.into()),
            border: Border {
                radius: (structure.small_gap / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        })
    ]
    .spacing(structure.gap);

    for room in sorted_rooms {
        column = column.push(room.render_icon(icon_size, avatar_cache));
    }

    floating_tile(theme, structure, column)
        .width(structure.server_column_width())
        .height(Fill)
        .padding(structure.small_gap * 1.5)
        .into()
}
