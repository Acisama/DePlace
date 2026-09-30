use std::collections::BTreeSet;

use deplace_core::{
    settings::NameDecoration,
    state::{ActiveServer, PresenceMap},
};
use iced::{Alignment, border::Radius};
use macros::{iced_cache, iced_icon};

use crate::{
    common::*,
    components::{HelpView, home::SidebarHelpKey},
};

#[derive(Debug, Clone)]
pub enum ChannelsMessage {
    None,
    NeedsAvatar(OwnedMxcUri),
    SetActiveRoom(DePlaceRoom),
    ToggleCategory(OwnedRoomId),
    RoomHovered(OwnedRoomId),
    RoomUnhovered(OwnedRoomId),
    HelpHover(Option<HelpKey>),
    OpenQuickselect,
}

impl NeedsAvatarExt for ChannelsMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ChannelsMessage::NeedsAvatar(uri)
    }
}

pub enum ChannelsAction {
    SetActiveRoom(DePlaceRoom),
    NeedsMedia(NeedsMedia),
    HelpHover(Option<HelpKey>),
    OpenQuickselect,
}

#[iced_cache(Clone)]
pub struct ServerChannels {
    state: AppState,
    avatar_cache: AvatarCache,

    room_watchers: RoomWatchers,

    #[hash]
    collapsed_categories: BTreeSet<OwnedRoomId>,

    #[hash]
    hovered_room_id: Option<OwnedRoomId>,

    name_decoration: Receiver<NameDecoration>,

    presence_map: Receiver<PresenceMap>,

    active_room: Receiver<Option<DePlaceRoom>>,
    active_server: Receiver<ActiveServer>,
}

impl ExtraHash for ServerChannels {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name_decoration.borrow().hash(state);
    }
}

impl ServerChannels {
    pub fn new(state: &AppState) -> Self {
        Self {
            avatar_cache: state.avatar_cache().clone(),
            room_watchers: state
                .room_watchers(hashing::hash_all_rooms_default())
                .clone(),

            collapsed_categories: BTreeSet::new(),

            presence_map: state.presence_map(),
            hovered_room_id: None,

            name_decoration: state.settings().name_decoration.watch(),

            active_room: state.active_room(),
            active_server: state.active_server(),

            avatar_states_for_hash: BTreeSet::new(),

            state: state.clone(),
        }
    }
}

impl IcedWidget<ChannelsMessage, ChannelsAction> for ServerChannels {
    fn update(&mut self, msg: ChannelsMessage) -> Option<ChannelsAction> {
        match msg {
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
            ChannelsMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(ChannelsAction::NeedsMedia(NeedsMedia::avatar(uri)))
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
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> Element<'static, ChannelsMessage> {
        let active_server = self.active_server.borrow().clone();
        let active_room_id = self
            .active_room
            .borrow()
            .as_ref()
            .map(|r| r.room_id().to_owned());

        let hovered_room_id = self.hovered_room_id.clone();

        let channels: Arc<Vec<_>> = match &active_server {
            ActiveServer::Dms => self.room_watchers.dm_rooms(),
            ActiveServer::Server(server) => Arc::new(
                self.room_watchers
                    .get_children(server.room_id())
                    .unwrap_or_default(),
            ),
        };

        let presence_map = &self.presence_map.borrow();
        let name_decoration = *self.name_decoration.borrow();

        let help_view = create_help_view(help_state, theme, ChannelsMessage::HelpHover);

        let (whole_key, name_key, column_key) = if active_server.is_dms() {
            (
                "The dm column",
                "You are in direct messages",
                "Your direct message conversations",
            )
        } else {
            (
                "The channels column",
                "The name of the active server",
                "Channels in this server",
            )
        };

        let room_list = Column::with_children(channels.iter().enumerate().map(|(i, r)| {
            render_channel(
                theme,
                structure,
                &active_room_id,
                &hovered_room_id,
                r,
                &self.avatar_cache,
                presence_map,
                &self.room_watchers,
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

        let room_list = help_view
            .call(
                HelpKey::Sidebar(SidebarHelpKey::ChannelsColumn),
                column_key,
                room_list,
            )
            .radius(Radius {
                top_left: 0.0,
                top_right: 0.0,
                bottom_left: structure.outer_border_radius,
                bottom_right: structure.outer_border_radius,
            });

        let help_view = create_help_view(help_state, theme, ChannelsMessage::HelpHover);

        let active_server_name = active_server.get_name();

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
                                    color: Some(theme.text.normal.into())
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
                                        w::button(iced_icon!(
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
                                            border: border::rounded(structure.inner_border_radius),
                                            ..Default::default()
                                        })
                                        .on_press(ChannelsMessage::OpenQuickselect)
                                        .padding(structure.small_gap / 2.0)
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
    room: &DePlaceRoom,
    avatar_cache: &AvatarCache,
    presence_map: &PresenceMap,
    room_watchers: &RoomWatchers,
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

    let text_color = if is_hovered || is_active {
        theme.text.normal.into()
    } else {
        theme.text.dim.into()
    };

    let bg = if is_active {
        theme.solid_hover_bg.into()
    } else {
        theme.solid_bg.into()
    };

    let is_space = room.is_space();

    let name = room.render_name_decorated(structure.font_size, name_decoration, text_color);

    let id = room.room_id().to_owned();

    let content: Element<'static, ChannelsMessage> = if is_space {
        let expanded = !collapsed_categories.contains(room.room_id());

        let mut column =
            w::column![
            w::mouse_area(
                w::button(
                    help_view.call(
                        HelpKey::Sidebar(SidebarHelpKey::Channel(index)),
                        "Channel",
                        w::row![
                            iced_icon!(expanded ? caret_down : caret_right, bold, icon_size,),
                            name,
                        ]
                        .spacing(structure.gap)
                        .width(Fill)
                        .align_y(Alignment::Center)
                        .padding(
                            padding::vertical(structure.small_gap * 0.75).left(structure.small_gap)
                        )
                    ).radius(structure.inner_border_radius)
                )
                .on_press(ChannelsMessage::ToggleCategory(room.room_id().to_owned()))
                .padding(0.0)
                .style(move |_, status| ButtonStyle {
                    background: None,
                    border: Border {
                        color: if status.active() {
                            theme.border.into()
                        } else {
                            Color::TRANSPARENT
                        },
                        width: structure.border_thickness,
                        radius: structure.inner_border_radius.into()
                    },
                    text_color,
                    ..Default::default()
                })
            )
            .on_enter(ChannelsMessage::RoomHovered(id.clone()))
            .on_exit(ChannelsMessage::RoomUnhovered(id.clone()))
        ]
            .spacing(structure.divider_width);

        if let Some(children) = room_watchers.get_children(room.room_id()) {
            let item_height = icon_size + 1.5 * structure.small_gap;
            if expanded {
                let length = children.len();

                let line_height = (item_height + structure.divider_width) * length as f32;

                column = column.push(
                    w::container(w::stack([
                        w::Column::with_children(children.iter().map(|room| {
                            w::row![
                                w::space().width(icon_size),
                                render_channel(
                                    theme,
                                    structure,
                                    active_room_id,
                                    hovered_room_id,
                                    room,
                                    avatar_cache,
                                    presence_map,
                                    room_watchers,
                                    collapsed_categories,
                                    name_decoration,
                                    icon_size,
                                    help_view,
                                    usize::MAX,
                                )
                            ]
                            .into()
                        }))
                        .spacing(structure.divider_width)
                        .into(),
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
                        .into(),
                    ]))
                    .padding(padding::left(structure.small_gap)),
                );
            } else if let Some(active_id) = &active_room_id
                && let Some(active) = children.iter().find(|room| room.room_id() == active_id)
            {
                let line_height = item_height + structure.divider_width;

                column = column.push(
                    w::container(w::stack([
                        w::row![
                            w::space().width(icon_size),
                            render_channel(
                                theme,
                                structure,
                                active_room_id,
                                hovered_room_id,
                                active,
                                avatar_cache,
                                presence_map,
                                room_watchers,
                                collapsed_categories,
                                name_decoration,
                                icon_size,
                                help_view,
                                usize::MAX,
                            )
                        ]
                        .into(),
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
                        .into(),
                    ]))
                    .padding(padding::left(structure.small_gap)),
                );
            }
        }

        column.into()
    } else {
        help_view
            .call(
                HelpKey::Sidebar(SidebarHelpKey::Channel(index)),
                if room.is_dm() { "A dm" } else { "A channel" },
                w::row![
                    context_room_icon(
                        room,
                        icon_size,
                        theme,
                        structure,
                        presence_map,
                        avatar_cache,
                        bg
                    ),
                    name,
                ]
                .spacing(structure.gap)
                .align_y(Alignment::Center)
                .width(Fill)
                .padding(
                    padding::vertical(structure.small_gap * 0.75).horizontal(structure.small_gap),
                ),
            )
            .radius(structure.inner_border_radius)
            .into()
    };

    w::mouse_area(
        w::button(content)
            .padding(0.0)
            .style(move |_, status| {
                let selected = is_active
                    || matches!(status, button::Status::Hovered | button::Status::Pressed);
                ButtonStyle {
                    background: is_active.then_some(theme.solid_hover_bg.into()),
                    border: Border {
                        color: if selected && !is_space {
                            theme.border.into()
                        } else {
                            Default::default()
                        },
                        width: structure.border_thickness,
                        radius: structure.inner_border_radius.into(),
                    },
                    text_color,
                    ..Default::default()
                }
            })
            .on_press_maybe(if is_space {
                Some(ChannelsMessage::None)
            } else {
                (!is_active).then_some(ChannelsMessage::SetActiveRoom(room.clone()))
            }),
    )
    .on_enter(if !is_space {
        ChannelsMessage::RoomHovered(id.clone())
    } else {
        ChannelsMessage::None
    })
    .on_exit(if !is_space {
        ChannelsMessage::RoomUnhovered(id)
    } else {
        ChannelsMessage::None
    })
    .into()
}
