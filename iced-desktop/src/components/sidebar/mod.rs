use std::{
    collections::{BTreeSet, HashSet},
    time::{Duration, Instant},
};

use deplace_core::{
    RoomMap,
    state::{
        ActiveServer, ActiveServerId,
        cache::{AvatarCache, MediaState},
    },
};
use iced::{
    Padding,
    widget::{MouseArea, svg},
};
use matrix_sdk::ruma::{OwnedMxcUri, OwnedRoomId};
use tokio::sync::watch::Receiver;

use crate::common::*;

#[derive(Clone, Copy)]
struct PillAnimation {
    from: f32,
    to: f32,
    start: Instant,
}

const PILL_ANIM: Duration = Duration::from_millis(150);

impl PillAnimation {
    fn height(&self, now: Instant) -> f32 {
        let t = (now.duration_since(self.start).as_secs_f32() / PILL_ANIM.as_secs_f32())
            .clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t); // same smoothstep you already use in loading.wgsl
        self.from + (self.to - self.from) * eased
    }
}

#[derive(Clone, Debug)]
pub enum SidebarMessage {
    ChangeActiveRoom(Option<Room>),
    ChangeActiveServer(ActiveServer),
    // ActiveServerChange(Option<Room>),
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

            active_room: state.active_room(),
            active_server: state.active_server(),

            hovered_server: None,

            state,
            avatar_states_for_hash: BTreeSet::new(),
        }
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

        w::row![
            render_server_column(
                theme,
                structure,
                sorted_rooms,
                active_server,
                &self.hovered_server,
                self.state.avatar_cache()
            ),
            Space::new().width(structure.small_gap),
            floating_tile(theme, structure, text(active_room.get_name()))
        ]
        .into()
    }
}

fn pill(
    theme: Theme,
    structure: Structure,
    active: bool,
    hovered: bool,
    has_messages: bool,
    content: MouseArea<'static, SidebarMessage>,
) -> Stack<'static, SidebarMessage> {
    let height = if active {
        structure.server_column.icon_size
    } else if hovered {
        structure.server_column.icon_size / 2.0
    } else if has_messages {
        structure.small_gap / 2.0
    } else {
        return Stack::new().push(w::row![Space::new().width(structure.small_gap), content]);
    };

    let offset = (structure.server_column.icon_size - height) / 2.0;

    Stack::new()
        .push(w::row![Space::new().width(structure.small_gap), content])
        .push(w::column![
            Space::new().height(offset),
            w::container(Space::new())
                .width(structure.small_gap / 2.0)
                .height(height)
                .style(move |_| ContainerStyle {
                    background: Some(theme.pill_color.into()),
                    border: Border {
                        radius: (structure.small_gap / 4.0).into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
        ])
}

fn render_server_column(
    theme: Theme,
    structure: Structure,
    sorted_rooms: Vec<Room>,
    active_server: ActiveServer,
    hovered_server: &Option<ActiveServerId>,
    avatar_cache: &AvatarCache,
) -> Element<'static, SidebarMessage> {
    let icon_handle = iced::advanced::svg::Handle::from_memory(include_bytes!(
        "../../../../assets/deplace_icon.svg"
    ));
    let icon_size = structure.server_column.icon_size;

    let mut column = w::column![
        pill(
            theme,
            structure,
            active_server.is_dms(),
            hovered_server
                .as_ref()
                .map(|id| id.is_dms())
                .unwrap_or(false),
            false,
            w::mouse_area(svg(icon_handle).width(icon_size).height(icon_size))
                .interaction(Interaction::Pointer)
                .on_press(SidebarMessage::ChangeActiveServer(ActiveServer::Dms))
                .on_enter(SidebarMessage::ServerHovered(ActiveServerId::Dms))
                .on_exit(SidebarMessage::ServerHoverEnded(ActiveServerId::Dms))
        ),
        w::row![
            Space::new().width(structure.small_gap),
            w::container(
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
    ]
    .spacing(structure.gap);

    for room in sorted_rooms {
        let id = room.room_id().to_owned();

        column = column.push(pill(
            theme,
            structure,
            active_server.is_server(&id),
            hovered_server
                .as_ref()
                .map(|sid| sid.is_server(&id))
                .unwrap_or(false),
            false,
            w::mouse_area(room.render_icon(icon_size, avatar_cache))
                .interaction(Interaction::Pointer)
                .on_press(SidebarMessage::ChangeActiveServer(ActiveServer::Server(
                    room,
                )))
                .on_enter(SidebarMessage::ServerHovered(ActiveServerId::Server(
                    id.clone(),
                )))
                .on_exit(SidebarMessage::ServerHoverEnded(ActiveServerId::Server(id))),
        ));
    }

    floating_tile(theme, structure, column)
        .width(structure.server_column_width())
        .height(Fill)
        .padding(Padding {
            top: structure.small_gap * 1.5,
            bottom: structure.small_gap / 2.0,
            left: structure.small_gap / 2.0,
            right: structure.small_gap / 2.0,
        })
        .into()
}
