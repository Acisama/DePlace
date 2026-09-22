use std::collections::{BTreeSet, HashSet};

use deplace_core::{
    matrix_api::sync::ParentToChildren,
    state::{ActiveServer, ActiveServerId, DmRoomMap},
};
use iced::widget::svg;
use macros::iced_cache;

use super::pill::PillCanvas;
use crate::common::*;

#[derive(Clone, Debug)]
pub enum ServerColumnMessage {
    ChangeActiveServer(ActiveServer),
    ChangeToDm(Room),
    NeedsAvatar(OwnedMxcUri),
    ServerHovered(ActiveServerId),
    ServerHoverEnded(ActiveServerId),
}

impl NeedsAvatarExt for ServerColumnMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ServerColumnMessage::NeedsAvatar(uri)
    }
}

pub enum ServerColumnAction {
    SetActiveServer(ActiveServer),
    SetActiveDm(Room),
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone)]
pub struct ServerColumn {
    state: AppState,

    server_rooms: Receiver<RoomMap>,
    server_order: Receiver<Vec<OwnedRoomId>>,
    parent_to_all_children: Receiver<ParentToChildren>,
    membership_map: Receiver<MembershipMap>,

    dm_rooms: Receiver<DmRoomMap>,

    #[hash]
    hovered_server: Option<ActiveServerId>,

    active_server: Receiver<ActiveServer>,

    avatar_cache: AvatarCache,
}

impl ExtraHash for ServerColumn {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for room in self.dm_rooms.borrow().values() {
            let count = room.unread_notification_counts();
            count.highlight_count.hash(state);
            count.notification_count.hash(state);
        }
    }
}

impl ServerColumn {
    pub fn new(state: &AppState) -> Self {
        Self {
            server_rooms: state.server_rooms(),
            server_order: state.server_order(),
            parent_to_all_children: state.parent_to_all_children(),
            membership_map: state.membership_map(),

            dm_rooms: state.dm_rooms(),

            hovered_server: None,
            active_server: state.active_server(),

            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),

            state: state.clone(),
        }
    }
}

impl IcedWidget<ServerColumnMessage, ServerColumnAction> for ServerColumn {
    fn update(&mut self, message: ServerColumnMessage) -> Option<ServerColumnAction> {
        match message {
            ServerColumnMessage::ChangeToDm(room) => Some(ServerColumnAction::SetActiveDm(room)),
            ServerColumnMessage::ChangeActiveServer(server) => {
                Some(ServerColumnAction::SetActiveServer(server))
            }
            ServerColumnMessage::ServerHovered(server) => {
                self.hovered_server = Some(server);
                None
            }
            ServerColumnMessage::ServerHoverEnded(server) => {
                if self.hovered_server == Some(server) {
                    self.hovered_server = None;
                }
                None
            }
            ServerColumnMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(ServerColumnAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ServerColumnMessage> {
        let hovered_server = self.hovered_server.clone();
        let avatar_cache = &self.avatar_cache;

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

        let mut sorted_rooms: IndexMap<OwnedRoomId, Room> = self
            .server_order
            .borrow()
            .iter()
            .filter_map(|id| rooms_map.get(id).map(|r| (id.clone(), r.clone())))
            .collect();

        let mut unsorted_rooms: IndexMap<OwnedRoomId, Room> = unsorted_server_ids
            .iter()
            .filter_map(|id| rooms_map.get(id).map(|r| (id.clone(), r.clone())))
            .collect();
        unsorted_rooms.sort_by_key(|id, _| id.clone());
        sorted_rooms.extend(unsorted_rooms);

        let icon_handle = iced::advanced::svg::Handle::from_memory(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/deplace_icon.svg"
        )));
        let icon_size = structure.server_column.icon_size;

        let membership_map = self.membership_map.borrow();

        let column = w::column![pill(
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
                .on_press(ServerColumnMessage::ChangeActiveServer(ActiveServer::Dms))
                .on_enter(ServerColumnMessage::ServerHovered(ActiveServerId::Dms))
                .on_exit(ServerColumnMessage::ServerHoverEnded(ActiveServerId::Dms))
        )]
        .extend(self.dm_rooms.borrow().iter().filter_map(|(id, room)| {
            let notifications = room.unread_notification_counts();

            let num = if notifications.highlight_count > 0 {
                Some(notifications.highlight_count)
            } else if notifications.notification_count > 0 {
                Some(notifications.notification_count)
            } else {
                None
            };

            num.map(|notif| {
                let hovered = hovered_server
                    .as_ref()
                    .map(|server_id| server_id.is_server(id))
                    .unwrap_or(false);

                let icon = if let Some(other_member) = room.get_other_member(&membership_map) {
                    other_member.render_icon(icon_size, avatar_cache)
                } else {
                    room.render_icon(icon_size, avatar_cache)
                };

                pill(
                    theme,
                    structure,
                    active_server.is_dms(),
                    hovered,
                    true,
                    w::mouse_area(
                        w::button(icon)
                            .padding(0.0)
                            .style(move |_, _| ButtonStyle {
                                ..Default::default()
                            })
                            .on_press(ServerColumnMessage::ChangeToDm(room.clone())),
                    )
                    .on_enter(ServerColumnMessage::ServerHovered(ActiveServerId::Server(
                        id.clone(),
                    )))
                    .on_exit(ServerColumnMessage::ServerHoverEnded(
                        ActiveServerId::Server(id.clone()),
                    )),
                )
                .into()
            })
        }))
        .push(w::row![
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
        ])
        .extend(sorted_rooms.into_iter().map(|(id, room)| {
            pill(
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
                    .on_press(ServerColumnMessage::ChangeActiveServer(
                        ActiveServer::Server(room),
                    ))
                    .on_enter(ServerColumnMessage::ServerHovered(ActiveServerId::Server(
                        id.clone(),
                    )))
                    .on_exit(ServerColumnMessage::ServerHoverEnded(
                        ActiveServerId::Server(id),
                    )),
            )
            .into()
        }))
        .spacing(structure.gap);

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
}

fn pill(
    theme: Theme,
    structure: Structure,
    active: bool,
    hovered: bool,
    has_messages: bool,
    content: impl Into<Element<'static, ServerColumnMessage>>,
) -> Stack<'static, ServerColumnMessage> {
    let target = if active {
        structure.server_column.icon_size
    } else if hovered {
        structure.server_column.icon_size / 2.0
    } else if has_messages {
        structure.small_gap / 2.0
    } else {
        0.0
    };

    let content = content.into();

    Stack::new()
        .push(w::row![Space::new().width(structure.small_gap), content])
        .push(w::column![
            Canvas::new(PillCanvas {
                target,
                width: structure.small_gap / 2.0,
                color: theme.pill_color.into(),
                radius: structure.small_gap / 4.0,
            })
            .width(structure.small_gap / 2.0)
            .height(structure.server_column.icon_size)
        ])
}
