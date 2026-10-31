use std::collections::{BTreeSet, HashSet};

use deplace_core::{
    matrix_api::account_data::set_account_data,
    rooms::NotificationCounts,
    state::{ActiveServer, ActiveServerId, PresenceMap},
};
use iced::widget::svg;
use macros::iced_cache;
use sweeten::widget::drag::DragEvent;

use super::pill::PillCanvas;
use crate::{
    common::*,
    components::help_mode::SidebarHelpKey,
    modules::home::{SPEAKER_ICON, chat::render_member_numbers},
};

#[derive(Clone, Debug)]
pub enum ServerColumnMessage {
    ChangeActiveServer(ActiveServer),
    ChangeToDm(DePlaceRoom),
    NeedsAvatar(OwnedMxcUri),
    ServerHovered(ActiveServerId),
    ServerHoverEnded(ActiveServerId),
    ServerDragged(DragEvent),
    HelpHover(Option<HelpKey>),
}

impl NeedsAvatarExt for ServerColumnMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ServerColumnMessage::NeedsAvatar(uri)
    }
}

impl HelpOverMessage for ServerColumnMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        ServerColumnMessage::HelpHover(help)
    }
}

pub enum ServerColumnAction {
    SetActiveServer(ActiveServer),
    SetActiveDm(DePlaceRoom),
    NeedsMedia(NeedsMedia),
    Run(Task<()>),
    HelpHover(Option<HelpKey>),
}

#[iced_cache(Clone)]
pub struct ServerColumn {
    state: AppState,

    room_watchers: RoomWatchers,

    #[hash]
    hovered_server: Option<ActiveServerId>,

    active_room: Receiver<Option<DePlaceRoom>>,
    active_server: Receiver<ActiveServer>,

    avatar_cache: AvatarCache,
    presence_map: Receiver<PresenceMap>,
    membership_map: Receiver<MembershipMap>,
}

impl ServerColumn {
    pub fn new(state: &AppState) -> Self {
        Self {
            room_watchers: state
                .room_watchers(
                    hashing::hash_dms_default()
                        .hash_servers_default()
                        .hash_all_rooms(hashing::hash_option_room(
                            hashing::hash_notifications().hash_call_state(true),
                        )),
                )
                .clone(),

            hovered_server: None,
            active_server: state.active_server(),

            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),

            active_room: state.active_room(),

            presence_map: state.presence_map(),
            membership_map: state.membership_map(),
            state: state.clone(),
        }
    }
}

impl IcedWidget<ServerColumnMessage, ServerColumnAction> for ServerColumn {
    fn update(&mut self, message: ServerColumnMessage) -> Option<ServerColumnAction> {
        match message {
            ServerColumnMessage::HelpHover(key) => Some(ServerColumnAction::HelpHover(key)),
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
            ServerColumnMessage::ServerDragged(drag) => {
                if let DragEvent::Dropped {
                    index,
                    target_index,
                } = drag
                {
                    let new_order = self.room_watchers.reorder_servers(index, target_index);
                    let client = self.state.client();
                    Some(ServerColumnAction::Run(Task::future(async move {
                        set_account_data(&client, new_order).await
                    })))
                } else {
                    None
                }
            }
        }
    }

    fn view(&self) -> Element<'static, ServerColumnMessage> {
        let theme = theme();
        let structure = structure();

        let hovered_server = self.hovered_server.clone();
        let avatar_cache = &self.avatar_cache;

        let active_server = self.active_server.borrow().clone();

        let icon_handle = iced::advanced::svg::Handle::from_memory(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/deplace_icon.svg"
        )));
        let icon_size = structure.server_column.icon_size;

        let help_view = HelpView::new(theme);

        let home_icon = pill(
            theme,
            structure,
            active_server.is_dms(),
            hovered_server
                .as_ref()
                .map(|id| id.is_dms())
                .unwrap_or(false),
            false,
            help_view
                .call(
                    SidebarHelpKey::HomeIcon,
                    "Go to your direct messages",
                    w::mouse_area(svg(icon_handle).width(icon_size).height(icon_size))
                        .interaction(Interaction::Pointer)
                        .on_press(ServerColumnMessage::ChangeActiveServer(ActiveServer::Dms))
                        .on_enter(ServerColumnMessage::ServerHovered(ActiveServerId::Dms))
                        .on_exit(ServerColumnMessage::ServerHoverEnded(ActiveServerId::Dms)),
                )
                .radius(icon_size / 2.0),
        );

        let dm_notification_pills: Vec<Element<'static, ServerColumnMessage>> = self
            .room_watchers
            .dm_rooms()
            .iter()
            .filter_map(|room| {
                let id = room.room_id().to_owned();

                let notifications = room.notification_counts();

                let has_call = room.has_call_participants();

                let num =
                    if notifications.highlight_count > 0 || notifications.notification_count > 0 {
                        Some(
                            notifications
                                .highlight_count
                                .max(notifications.notification_count),
                        )
                    } else {
                        None
                    };

                if num.is_none() && !has_call {
                    return None;
                }

                let mut badge = corner_badge(
                    help_view.call(
                        HelpKey::Sidebar(SidebarHelpKey::DmsWithNotifications),
                        "Direct messages with unread notifications or ongoing calls",
                        w::mouse_area(
                            w::button(room.render_icon(icon_size, avatar_cache))
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
                    ),
                    0.4,
                    structure.icon_gap,
                    theme.solid_bg,
                );

                let own_user_in_call = room.own_user_is_in_call();
                if has_call {
                    badge = badge.tr(CornerContent::Icon {
                        svg_content: SPEAKER_ICON,
                        color: if own_user_in_call {
                            theme.solid_bg.into()
                        } else {
                            theme.text.normal.into()
                        },
                        background: if own_user_in_call {
                            theme.colors.success.into()
                        } else {
                            theme.colors.offline.into()
                        },
                    })
                }

                if let Some(notif) = num {
                    badge = badge.br(CornerContent::text(
                        notif.to_string(),
                        theme.solid_bg,
                        theme.accent,
                    ))
                }

                let hovered = hovered_server
                    .as_ref()
                    .map(|server_id| server_id.is_server(&id))
                    .unwrap_or(false);

                Some(
                    pill(
                        theme,
                        structure,
                        self.active_room.borrow().as_ref().map(|r| r.room_id())
                            == Some(room.room_id()),
                        hovered,
                        num.is_some(),
                        badge,
                    )
                    .into(),
                )
            })
            .collect();

        let mut column = w::column![home_icon];

        if !dm_notification_pills.is_empty() {
            column = column.push(w::column(dm_notification_pills).spacing(structure.gap));
        }

        let presence_map = self.presence_map.borrow();
        let membership_map = self.membership_map.borrow();

        let servers = sweeten::widget::column(self.room_watchers.servers().iter().enumerate().map(
            |(index, room)| {
                render_server_icon(
                    room,
                    index,
                    theme,
                    structure,
                    &self.room_watchers,
                    &self.avatar_cache,
                    &active_server,
                    &self.hovered_server,
                    &help_view,
                    &presence_map,
                    &membership_map,
                )
            },
        ))
        .on_drag(ServerColumnMessage::ServerDragged)
        .spacing(structure.gap);

        let column = column
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
            .push(servers)
            .spacing(structure.gap);

        help_view
            .call(
                HelpKey::Sidebar(SidebarHelpKey::ServerColumn),
                "The server column, switch between servers and direct messages here",
                floating_tile(theme, structure, column)
                    .width(structure.server_column_width())
                    .height(Fill)
                    .padding(Padding {
                        top: structure.small_gap * 1.5,
                        bottom: structure.small_gap / 2.0,
                        left: structure.small_gap / 2.0,
                        right: structure.small_gap / 2.0,
                    }),
            )
            .radius(structure.outer_border_radius)
            .into()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_server_icon(
    room: &DePlaceRoom,
    index: usize,
    theme: Theme,
    structure: Structure,
    room_watchers: &RoomWatchers,
    avatar_cache: &AvatarCache,
    active_server: &ActiveServer,
    hovered_server: &Option<ActiveServerId>,
    help_view: &HelpView<ServerColumnMessage>,
    presence_map: &PresenceMap,
    membership_map: &MembershipMap,
) -> Element<'static, ServerColumnMessage> {
    let id = room.room_id().to_owned();
    let children = room_watchers.get_all_children(&id);

    let mut online_members = 0;
    let mut offline_members = 0;

    let mut user_ids = HashSet::new();

    for child in &children {
        if let Some(map) = membership_map.get(child.room_id()) {
            for id in map.keys() {
                user_ids.insert(id.clone());
            }
        }
    }

    for id in user_ids {
        if let Some(presence) = presence_map.get(&id)
            && presence.presence != PresenceState::Offline
        {
            online_members += 1;
        } else {
            offline_members += 1;
        }
    }

    let has_call = children.iter().any(|r| r.has_call_participants());
    let own_user_in_call = children.iter().any(|r| r.own_user_is_in_call());

    let counts: NotificationCounts = children.iter().map(|r| r.notification_counts()).sum();
    let icon_size = structure.server_column.icon_size;

    let content = w::mouse_area(themed_tooltip_content(
        room.render_icon(icon_size, avatar_cache),
        w::column![
            weighted_text(room.get_name(), iced::font::Weight::Bold).size(structure.font_size),
            render_member_numbers(
                online_members,
                offline_members,
                theme,
                structure,
                help_view,
                false
            )
        ],
        structure,
        theme,
        w::tooltip::Position::Right,
        structure.small_gap * 1.5 - structure.border_thickness,
    ))
    .interaction(Interaction::Pointer)
    .on_release(ServerColumnMessage::ChangeActiveServer(
        ActiveServer::Server(room.clone()),
    ))
    .on_enter(ServerColumnMessage::ServerHovered(ActiveServerId::Server(
        id.clone(),
    )))
    .on_exit(ServerColumnMessage::ServerHoverEnded(
        ActiveServerId::Server(id.clone()),
    ));

    let mut badge = corner_badge(content, 0.4, 0.15, theme.solid_bg);

    if let Some(highlights) = counts.highlights() {
        badge = badge.br(CornerContent::text(
            highlights.to_string(),
            theme.solid_bg,
            theme.accent,
        ))
    }

    if has_call {
        badge = badge.tr(CornerContent::Icon {
            svg_content: SPEAKER_ICON,
            color: if own_user_in_call {
                theme.solid_bg.into()
            } else {
                theme.text.normal.into()
            },
            background: if own_user_in_call {
                theme.colors.success.into()
            } else {
                theme.colors.offline.into()
            },
        })
    }

    pill(
        theme,
        structure,
        active_server.is_server(&id),
        hovered_server
            .as_ref()
            .map(|sid| sid.is_server(&id))
            .unwrap_or(false),
        counts.has_notifications(),
        help_view
            .call(
                SidebarHelpKey::Server(index),
                "A server, click on it to view it",
                badge,
            )
            .radius(icon_size * room.icon_border_radius_ratio()),
    )
    .into()
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

    // `content` must stay the base layer (first `.push`) so it drives the
    // Stack's size -- pushing the canvas first shrinks the whole pill down
    // to the canvas's tiny fixed size instead. `push_under` puts the canvas
    // behind content for both drawing and `Stack::update`'s topmost-first,
    // stop-on-capture traversal, so an ancestor that captures pointer events
    // before forwarding (e.g. an active help-mode target) still reaches
    // content first, same as `floating_tile` does for its background layer.
    Stack::new()
        .push(w::row![Space::new().width(structure.small_gap), content])
        .push_under(w::column![
            Canvas::new(PillCanvas {
                target,
                width: structure.small_gap / 2.0,
                color: theme.text.normal.into(),
                radius: structure.small_gap / 4.0,
            })
            .width(structure.small_gap / 2.0)
            .height(structure.server_column.icon_size)
        ])
}
