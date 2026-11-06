use std::collections::BTreeSet;

use deplace_core::{room::call::CallMember, settings::NameDecoration, state::ActiveServer};
use iced::{Alignment, border::Radius};
use macros::{iced_cache, iced_icon};

use crate::{
    common::*,
    components::{
        PushElement,
        help_mode::SidebarHelpKey,
        link::link,
        phosphor_icon,
        profile::{OpenProfileOverlayExt, RoomRenderExt},
        volume_bar::{self, volume_bar},
    },
    modules::home::MUTE_ICON,
};

use super::render_mention_badge;

#[derive(Debug, Clone, Default)]
pub enum ChannelsMessage {
    #[default]
    None,
    SetActiveRoom(DePlaceRoom),
    ToggleCategory(OwnedRoomId),
    RoomHovered(OwnedRoomId),
    RoomUnhovered(OwnedRoomId),
    HelpHover(Option<HelpKey>),
    OpenQuickselect,
    ShowProfile {
        member: Member,
        bounds: Rectangle,
    },
    CallParticipantHovered {
        room_id: OwnedRoomId,
        participant: CallMember,
    },
    CallParticipantUnhovered {
        room_id: OwnedRoomId,
        participant: CallMember,
    },
    JoinCall(DePlaceRoom),
}

impl OpenProfileOverlayExt for ChannelsMessage {
    fn open_profile(member: Member, bounds: Rectangle) -> Self {
        Self::ShowProfile { member, bounds }
    }
}

impl HelpOverMessage for ChannelsMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        ChannelsMessage::HelpHover(help)
    }
}

pub enum ChannelsAction {
    SetActiveRoom(DePlaceRoom),
    HelpHover(Option<HelpKey>),
    OpenQuickselect,
    ShowProfile { member: Member, bounds: Rectangle },
    JoinCall(DePlaceRoom),
}

#[iced_cache(Clone)]
pub struct ServerChannels {
    room_store: Arc<RoomStore>,

    #[hash]
    collapsed_categories: BTreeSet<OwnedRoomId>,

    #[hash]
    hovered_room_id: Option<OwnedRoomId>,

    #[hash]
    hovered_call_participant: Option<(OwnedRoomId, CallMember)>,

    name_decoration: Receiver<NameDecoration>,

    active_room: Receiver<Option<DePlaceRoom>>,
    active_server: Receiver<ActiveServer>,
}

impl ExtraHash for ServerChannels {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name_decoration.borrow().hash(state);

        for room in self.room_store.dm_rooms().iter() {
            room.hash_all(state);
        }

        if let Some(server) = self.active_server.borrow().as_server() {
            for room in self.room_store.get_all_children(server.room_id()) {
                room.hash_selective(state, true, true, false, true);
            }
        }
    }
}

impl ServerChannels {
    pub fn new(state: &AppState) -> Self {
        Self {
            room_store: state.room_store(),

            collapsed_categories: BTreeSet::new(),

            hovered_room_id: None,
            hovered_call_participant: None,

            name_decoration: state.settings().name_decoration.watch(),

            active_room: state.active_room(),
            active_server: state.active_server(),
        }
    }
}

impl IcedWidget<ChannelsMessage, ChannelsAction> for ServerChannels {
    fn update(&mut self, msg: ChannelsMessage) -> Option<ChannelsAction> {
        match msg {
            ChannelsMessage::JoinCall(room) => Some(ChannelsAction::JoinCall(room)),
            ChannelsMessage::OpenQuickselect => Some(ChannelsAction::OpenQuickselect),
            ChannelsMessage::HelpHover(key) => Some(ChannelsAction::HelpHover(key)),
            ChannelsMessage::RoomHovered(id) => {
                self.hovered_room_id = Some(id);
                None
            }
            ChannelsMessage::RoomUnhovered(id) => {
                if self.hovered_room_id.as_ref() == Some(&id) {
                    self.hovered_room_id = None;
                }
                None
            }
            ChannelsMessage::None => None,
            ChannelsMessage::SetActiveRoom(room) => Some(ChannelsAction::SetActiveRoom(room)),
            ChannelsMessage::ToggleCategory(id) => {
                if self.collapsed_categories.contains(&id) {
                    self.collapsed_categories.remove(&id);
                } else {
                    self.collapsed_categories.insert(id);
                }
                None
            }
            ChannelsMessage::ShowProfile { member, bounds } => {
                Some(ChannelsAction::ShowProfile { member, bounds })
            }
            ChannelsMessage::CallParticipantHovered {
                room_id,
                participant,
            } => {
                self.hovered_call_participant = Some((room_id, participant));
                None
            }
            ChannelsMessage::CallParticipantUnhovered {
                room_id,
                participant,
            } => {
                if self.hovered_call_participant == Some((room_id, participant)) {
                    self.hovered_call_participant = None;
                }
                None
            }
        }
    }

    fn view(&self) -> Element<'static, ChannelsMessage> {
        let theme = theme();
        let structure = structure();

        let active_server = self.active_server.borrow().clone();
        let active_room_id = self
            .active_room
            .borrow()
            .as_ref()
            .map(|r| r.room_id().to_owned());

        let hovered_room_id = self.hovered_room_id.clone();
        let hovered_call_participant = self.hovered_call_participant.clone();

        let channels: Arc<Vec<_>> = match &active_server {
            ActiveServer::Dms => self.room_store.dm_rooms(),
            ActiveServer::Server(server) => Arc::new(
                self.room_store
                    .get_children(server.room_id())
                    .unwrap_or_default(),
            ),
        };

        let room_store = &self.room_store;
        let name_decoration = *self.name_decoration.borrow();
        let help_view = HelpView::new(theme);

        let (whole_key, name_key) = if active_server.is_dms() {
            (
                "An view of your direct message conversations",
                "You are in direct messages",
            )
        } else {
            (
                "An view of channels in the active server",
                "The name of the active server",
            )
        };

        let room_list = Column::with_children(channels.iter().enumerate().map(|(i, r)| {
            render_channel(
                theme,
                structure,
                &active_room_id,
                &hovered_room_id,
                &hovered_call_participant,
                r,
                room_store,
                &self.collapsed_categories,
                name_decoration,
                if r.is_dm() {
                    structure.sidebar.dm_icon_height
                } else {
                    structure.sidebar.channel_icon_height
                },
                &help_view,
                i,
            )
        }))
        .spacing(structure.divider_width)
        .height(Fill)
        .padding(structure.small_gap);

        let active_server_name = active_server.displayname();

        let content = floating_tile(
            theme,
            structure,
            w::column![
                help_view
                    .call(
                        HelpKey::Sidebar(SidebarHelpKey::ServerName),
                        name_key,
                        w::row![
                            w::container(pan(weighted_text(active_server_name, Weight::Bold)
                                .size(structure.large_font_size)
                                .style(move |_| TextStyle {
                                    color: Some(theme.text.normal.into()),
                                    selection: Some(theme.accent.into())
                                })
                                .wrapping(text::Wrapping::None)
                                .center()
                                .align_x(w::text::Alignment::Left)
                                .height(Fill)))
                            .padding(padding::left(
                                (structure.header.height - structure.large_font_size) / 2.0
                            ))
                            .width(Fill)
                            .height(Fill),
                            w::container(
                                help_view
                                    .call(
                                        HelpKey::Sidebar(SidebarHelpKey::QuickselectButton),
                                        "Quickselect button, press to open quickselect",
                                        themed_tooltip_text(
                                            animated_button(iced_icon!(
                                                compass_rose,
                                                fill,
                                                structure.header.height - structure.small_gap * 3.0
                                            ))
                                            .style(move |_, status| ButtonStyle {
                                                background: status
                                                    .active()
                                                    .then_some(theme.solid_hover_bg.into()),
                                                text_color: if status.active() {
                                                    theme.text.normal.into()
                                                } else {
                                                    theme.text.dim.into()
                                                },
                                                border: border::rounded(
                                                    structure.inner_border_radius
                                                ),
                                                ..Default::default()
                                            })
                                            .on_press(ChannelsMessage::OpenQuickselect)
                                            .padding(structure.small_gap / 2.0),
                                            "Open quick select",
                                            structure,
                                            theme
                                        )
                                    )
                                    .radius(structure.inner_border_radius)
                            )
                            .padding(structure.small_gap)
                        ]
                        .height(structure.header.height),
                    )
                    .radius(Radius {
                        top_left: structure.outer_border_radius,
                        top_right: structure.outer_border_radius,
                        bottom_left: 0.0,
                        bottom_right: 0.0,
                    }),
                w::container(Space::new())
                    .width(Fill)
                    .height(structure.border_thickness)
                    .style(move |_| ContainerStyle::default().background(theme.border)),
                room_list
            ],
        )
        .width(structure.sidebar.width)
        .height(Fill);

        help_view
            .call(
                HelpKey::Sidebar(SidebarHelpKey::Channels),
                whole_key,
                content,
            )
            .radius(structure.outer_border_radius)
            .into()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_channel(
    theme: Theme,
    structure: Structure,
    active_room_id: &Option<OwnedRoomId>,
    hovered_room_id: &Option<OwnedRoomId>,
    hovered_call_participant: &Option<(OwnedRoomId, CallMember)>,
    room: &DePlaceRoom,
    room_store: &RoomStore,
    collapsed_categories: &BTreeSet<OwnedRoomId>,
    name_decoration: NameDecoration,
    icon_size: f32,
    help_view: &HelpView<ChannelsMessage>,
    index: usize,
) -> Element<'static, ChannelsMessage> {
    let is_active = active_room_id
        .as_ref()
        .is_some_and(|id| id == room.room_id());

    let is_hovered = hovered_room_id
        .as_ref()
        .is_some_and(|id| id == room.room_id());

    let live = room.live();

    let notifications = live.notification_counts();
    let has_notifications = notifications.has_notifications();

    let text_color = if is_hovered || is_active || has_notifications {
        theme.text.normal.into()
    } else {
        theme.text.dim.into()
    };

    let bg = if is_active {
        theme.solid_hover_bg.into()
    } else {
        theme.solid_bg.into()
    };

    let is_space = room.room_type().is_space();

    let name = room.render_name_decorated(structure.font_size, name_decoration, text_color);

    let id_ref = room.room_id();
    let id = id_ref.to_owned();

    let expanded = !collapsed_categories.contains(room.room_id());
    let is_active = active_room_id.as_ref().is_some_and(|id| id == id_ref);

    let can_join_call = room.room_type().is_call() && !room.call().own_user_device_is_in_call();

    let area = w::mouse_area(
        w::button(
            help_view
                .call(
                    HelpKey::Sidebar(SidebarHelpKey::Channel(index)),
                    "Channel",
                    w::row![
                        if is_space {
                            if expanded {
                                iced_icon!(caret_down, bold, icon_size).into()
                            } else {
                                iced_icon!(caret_right, bold, icon_size).into()
                            }
                        } else {
                            room.render_simple_icon(icon_size, theme, structure, bg)
                        },
                        name,
                    ]
                    .spacing(structure.gap)
                    .width(Fill)
                    .align_y(Alignment::Center)
                    .padding(
                        padding::vertical(structure.small_gap * 0.75).left(structure.small_gap),
                    ),
                )
                .radius(structure.inner_border_radius),
        )
        .on_press_maybe(if !is_active || is_space {
            if is_space {
                Some(ChannelsMessage::ToggleCategory(room.room_id().to_owned()))
            } else {
                Some(ChannelsMessage::SetActiveRoom(room.clone()))
            }
        } else if can_join_call {
            Some(ChannelsMessage::JoinCall(room.clone()))
        } else {
            None
        })
        .padding(0.0)
        .style(move |_, status| ButtonStyle {
            background: is_active.then_some(theme.solid_hover_bg.into()),
            border: Border {
                color: if status.active() || is_active {
                    theme.border.into()
                } else {
                    Color::TRANSPARENT
                },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            text_color,
            ..Default::default()
        }),
    )
    .on_enter(ChannelsMessage::RoomHovered(id.clone()))
    .on_exit(ChannelsMessage::RoomUnhovered(id.clone()));

    let mut room_stack = w::stack![area.boxed()];

    let mentions = if room.is_dm() {
        let total = notifications.total();
        if total > 0 { Some(total) } else { None }
    } else {
        notifications.highlights()
    };

    if let Some(mentions) = mentions {
        room_stack = room_stack.push_element(
            w::row![
                Space::new().width(Fill),
                render_mention_badge(mentions, theme, structure),
                Space::new().width(structure.small_gap)
            ]
            .height(Fill)
            .width(Fill)
            .align_y(Alignment::Center),
        );
    }

    let mut items = vec![
        help_view
            .call(
                HelpKey::Sidebar(SidebarHelpKey::Channel(index)),
                if room.is_dm() { "A dm" } else { "A channel" },
                room_stack,
            )
            .radius(structure.inner_border_radius)
            .boxed(),
    ];

    if has_notifications && !room.is_dm() {
        items.push(
            w::container(
                w::container("")
                    .height(structure.small_gap)
                    .width(structure.small_gap / 2.0)
                    .style(move |_| ContainerStyle {
                        background: Some(theme.text.normal.into()),
                        border: border::rounded(structure.small_gap / 2.0),
                        ..Default::default()
                    }),
            )
            .width(structure.small_gap)
            .height(Fill)
            .center_y(Fill)
            .boxed(),
        );
    }

    let mut column = w::column![w::Stack::with_children(items)].spacing(structure.divider_width);

    if is_space && let Some(children) = room_store.get_children(room.room_id()) {
        let item_height = icon_size + 1.5 * structure.small_gap;
        if expanded {
            let length = children.len();

            let line_height = (item_height + structure.divider_width) * length as f32;

            column = column.push_element(
                w::container(w::stack([
                    w::Column::with_children(children.iter().map(|room| {
                        w::row![
                            w::space().width(icon_size),
                            render_channel(
                                theme,
                                structure,
                                active_room_id,
                                hovered_room_id,
                                hovered_call_participant,
                                room,
                                room_store,
                                collapsed_categories,
                                name_decoration,
                                icon_size,
                                help_view,
                                usize::MAX,
                            )
                        ]
                    }))
                    .spacing(structure.divider_width)
                    .boxed(),
                    w::container(
                        w::container("")
                            .width(structure.divider_width)
                            .height(line_height)
                            .style(move |_| ContainerStyle {
                                background: Some(theme.border.into()),
                                border: border::rounded(structure.divider_width / 2.0),
                                ..Default::default()
                            }),
                    )
                    .width(icon_size)
                    .center_x(icon_size)
                    .boxed(),
                ]))
                .padding(padding::left(structure.small_gap)),
            );
        } else if let Some(active_id) = &active_room_id
            && let Some(active) = children.iter().find(|room| room.room_id() == active_id)
        {
            column = column.push_element(
                w::container(w::stack![
                    w::row![
                        w::space().width(icon_size),
                        render_channel(
                            theme,
                            structure,
                            active_room_id,
                            hovered_room_id,
                            hovered_call_participant,
                            active,
                            room_store,
                            collapsed_categories,
                            name_decoration,
                            icon_size,
                            help_view,
                            usize::MAX,
                        )
                    ],
                    w::container(
                        w::container("")
                            .width(structure.divider_width)
                            .height(icon_size)
                            .style(move |_| ContainerStyle {
                                background: Some(theme.border.into()),
                                border: border::rounded(structure.divider_width / 2.0),
                                ..Default::default()
                            }),
                    )
                    .width(icon_size)
                    .center_x(icon_size)
                ])
                .padding(padding::left(structure.small_gap)),
            );
        }

        column.boxed()
    } else {
        if !room.is_dm() {
            let item_height = icon_size + structure.small_gap;

            let participants = room.call().call_participants();
            let line_height = (item_height + structure.divider_width) * participants.len() as f32;

            let user_in_call = room.call().own_user_device_is_in_call();

            column
                .push(
                    w::container(w::stack![
                        w::Column::with_children(participants.into_iter().map(|p| {
                            let is_hovered = hovered_call_participant
                                .as_ref()
                                .is_some_and(|(r, u)| r == room.room_id() && u == &p);

                            render_call_participant(
                                p,
                                room.room_id(),
                                is_hovered,
                                theme,
                                structure,
                                icon_size,
                                user_in_call,
                            )
                        }))
                        .spacing(structure.divider_width)
                        .padding(padding::left(icon_size)),
                        w::container(
                            w::container("")
                                .width(structure.divider_width)
                                .height(line_height)
                                .style(move |_| ContainerStyle {
                                    background: Some(theme.border.into()),
                                    border: border::rounded(structure.divider_width / 2.0),
                                    ..Default::default()
                                }),
                        )
                        .width(icon_size)
                        .center_x(icon_size)
                        .boxed(),
                    ])
                    .padding(padding::left(structure.small_gap))
                    .boxed(),
                )
                .boxed()
        } else {
            column.boxed()
        }
    }
}

fn render_call_participant(
    member: CallMember,
    room_id: &RoomId,
    is_hovered: bool,
    theme: Theme,
    structure: Structure,
    icon_size: f32,
    user_in_call: bool,
) -> Element<'static, ChannelsMessage> {
    let room_id = room_id.to_owned();
    let hover_room_id = room_id.clone();

    let mut view_row = vec![member.render_small_icon(icon_size)];

    if user_in_call {
        view_row.push(volume_bar(
            member.volume(),
            icon_size / 4.0,
            icon_size,
            volume_bar::Direction::Top,
            Color::from_rgb8(255, 0, 0),
            Color::from_rgb8(0, 255, 0),
            theme.solid_bg.into(),
            structure.smaller_border_radius,
            theme.border.into(),
            structure.border_thickness,
        ));
    }

    view_row.push(member.render_name(structure.font_size));

    if user_in_call && member.is_muted() {
        view_row.push(Space::new().width(Fill).boxed());
        view_row.push(
            phosphor_icon(MUTE_ICON, icon_size * 0.8)
                .color(theme.text.dim)
                .boxed(),
        );
    }

    w::row![link(
        w::mouse_area(
            w::container(
                w::Row::with_children(view_row)
                    .spacing(structure.small_gap)
                    .align_y(Alignment::Center)
                    .width(Fill),
            )
            .padding(padding::vertical(structure.small_gap / 2.0).horizontal(structure.small_gap),)
            .style(move |_| ContainerStyle {
                background: is_hovered.then_some(theme.solid_hover_bg.into()),
                border: Border {
                    color: if is_hovered {
                        theme.border.into()
                    } else {
                        Color::TRANSPARENT
                    },
                    width: structure.border_thickness,
                    radius: structure.inner_border_radius.into(),
                },
                ..Default::default()
            }),
        )
        .on_enter(ChannelsMessage::CallParticipantHovered {
            room_id: hover_room_id.clone(),
            participant: member.clone(),
        })
        .on_exit(ChannelsMessage::CallParticipantUnhovered {
            room_id: hover_room_id,
            participant: member.clone(),
        }),
        Color::TRANSPARENT,
        move |bounds| ChannelsMessage::open_profile(member.member.clone(), bounds),
    )]
    .boxed()
}
