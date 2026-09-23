use std::collections::BTreeSet;

use deplace_core::{
    rooms::{NotificationCounts, hashing::RoomHashingConfig},
    state::{ActiveServer, ActiveServerId},
};
use iced::widget::svg;
use macros::iced_cache;

use super::pill::PillCanvas;
use crate::common::*;

#[derive(Clone, Debug)]
pub enum ServerColumnMessage {
    ChangeActiveServer(ActiveServer),
    ChangeToDm(DePlaceRoom),
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
    SetActiveDm(DePlaceRoom),
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone)]
pub struct ServerColumn {
    state: AppState,

    room_watchers: RoomWatchers,
    membership_map: Receiver<MembershipMap>,

    #[hash]
    hovered_server: Option<ActiveServerId>,

    active_server: Receiver<ActiveServer>,

    avatar_cache: AvatarCache,
}

// impl ExtraHash for ServerColumn {
//     fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
//         for room in self.dm_rooms.borrow().values() {
//             let count = room.unread_notification_counts();
//             count.highlight_count.hash(state);
//             count.notification_count.hash(state);
//         }

//         for id in self.server_order.borrow().iter() {
//             let ptac = self.parent_to_all_children.borrow();
//             let Some(rooms) = ptac.get(id) else {
//                 continue;
//             };
//             for room in rooms.values() {
//                 let count = room.unread_notification_counts();
//                 count.highlight_count.hash(state);
//                 count.notification_count.hash(state);
//             }
//         }
//     }
// }

impl ServerColumn {
    pub fn new(state: &AppState) -> Self {
        Self {
            room_watchers: state
                .room_watchers(
                    hashing::hash_dms_default()
                        .hash_servers_default()
                        .hash_all_rooms(hashing::hash_option_room(hashing::hash_notifications())),
                )
                .clone(),
            membership_map: state.membership_map(),

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
        .extend(self.room_watchers.dm_rooms().iter().filter_map(|room| {
            let id = room.room_id().to_owned();

            let notifications = room.notification_counts();

            let num = if notifications.highlight_count > 0 || notifications.notification_count > 0 {
                Some(
                    notifications
                        .highlight_count
                        .max(notifications.notification_count),
                )
            } else {
                None
            };

            num.map(|notif| {
                let hovered = hovered_server
                    .as_ref()
                    .map(|server_id| server_id.is_server(&id))
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
                    corner_badge(
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
                        0.4,
                        0.15,
                        theme.solid_bg,
                    )
                    .br(CornerContent::text(
                        notif.to_string(),
                        theme.solid_bg,
                        theme.accent,
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
        .extend(self.room_watchers.servers().iter().map(|room| {
            let id = room.room_id().to_owned();
            let children = self.room_watchers.get_all_children(&id);

            let counts: NotificationCounts = children.iter().map(|r| r.notification_counts()).sum();

            let content = w::mouse_area(room.render_icon(icon_size, avatar_cache))
                .interaction(Interaction::Pointer)
                .on_press(ServerColumnMessage::ChangeActiveServer(
                    ActiveServer::Server(room.clone()),
                ))
                .on_enter(ServerColumnMessage::ServerHovered(ActiveServerId::Server(
                    id.clone(),
                )))
                .on_exit(ServerColumnMessage::ServerHoverEnded(
                    ActiveServerId::Server(id.clone()),
                ));

            pill(
                theme,
                structure,
                active_server.is_server(&id),
                hovered_server
                    .as_ref()
                    .map(|sid| sid.is_server(&id))
                    .unwrap_or(false),
                counts.has_notifications(),
                if let Some(highlights) = counts.highlights() {
                    Element::from(corner_badge(content, 0.4, 0.15, theme.solid_bg).br(
                        CornerContent::text(highlights.to_string(), theme.solid_bg, theme.accent),
                    ))
                } else {
                    content.into()
                },
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
        structure.server_column.icon_size / 4.0
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
