use std::collections::BTreeSet;

use deplace_core::state::{ActiveServer, PresenceMap};
use iced::widget::text::Alignment;
use macros::{iced_cache, iced_icon};

use crate::common::*;

#[derive(Debug, Clone)]
pub enum ChannelsMessage {
    None,
    NeedsAvatar(OwnedMxcUri),
    SetActiveRoom(DePlaceRoom),
    ToggleCategory(OwnedRoomId),
}

impl NeedsAvatarExt for ChannelsMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ChannelsMessage::NeedsAvatar(uri)
    }
}

pub enum ChannelsAction {
    SetActiveRoom(DePlaceRoom),
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone)]
pub struct ServerChannels {
    state: AppState,
    avatar_cache: AvatarCache,

    room_watchers: RoomWatchers,

    #[hash]
    collapsed_categories: BTreeSet<OwnedRoomId>,

    presence_map: Receiver<PresenceMap>,

    active_room: Receiver<Option<DePlaceRoom>>,
    active_server: Receiver<ActiveServer>,
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

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ChannelsMessage> {
        let active_server = self.active_server.borrow().clone();
        let active_room_id = self
            .active_room
            .borrow()
            .as_ref()
            .map(|r| r.room_id().to_owned());

        let channels: Arc<Vec<_>> = match &active_server {
            ActiveServer::Dms => self.room_watchers.dm_rooms(),
            ActiveServer::Server(server) => Arc::new(
                self.room_watchers
                    .get_children(server.room_id())
                    .unwrap_or_default(),
            ),
        };

        let presence_map = &self.presence_map.borrow();

        floating_tile(
            theme,
            structure,
            w::column![
                w::container(
                    weighted_text(active_server.get_name(), Weight::Bold)
                        .size(structure.large_font_size)
                        .style(move |_| TextStyle {
                            color: Some(theme.text.normal.into())
                        })
                        .wrapping(text::Wrapping::None)
                        .center()
                        .width(Fill)
                        .align_x(Alignment::Left)
                        .height(Fill)
                )
                .padding(padding::horizontal(
                    (structure.header.height - structure.large_font_size) / 2.0
                ))
                .height(structure.header.height),
                w::container(Space::new())
                    .width(Fill)
                    .height(structure.border_thickness)
                    .style(move |_| ContainerStyle::default().background(theme.border)),
                Column::with_children(channels.iter().map(|r| {
                    render_channel(
                        theme,
                        structure,
                        &active_room_id,
                        r,
                        &self.avatar_cache,
                        presence_map,
                        &self.room_watchers,
                        &self.collapsed_categories,
                        if r.is_dm() {
                            structure.sidebar.dm_icon_height
                        } else {
                            structure.sidebar.channel_icon_height
                        },
                    )
                }))
                .spacing(structure.divider_width)
                .padding(structure.small_gap)
            ],
        )
        .width(structure.sidebar.width)
        .height(Fill)
        .into()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_channel(
    theme: Theme,
    structure: Structure,
    active_room_id: &Option<OwnedRoomId>,
    room: &DePlaceRoom,
    avatar_cache: &AvatarCache,
    presence_map: &PresenceMap,
    room_watchers: &RoomWatchers,
    collapsed_categories: &BTreeSet<OwnedRoomId>,
    icon_size: f32,
) -> Element<'static, ChannelsMessage> {
    let is_active = active_room_id
        .as_ref()
        .is_some_and(|id| id == room.room_id());

    let is_space = room.is_space();

    let name = room.get_name();

    let content: Element<'static, ChannelsMessage> = if is_space {
        let expanded = !collapsed_categories.contains(room.room_id());

        let mut column = w::column![
            w::button(
                w::row![
                    iced_icon!(expanded ? caret_down : caret_right, bold, icon_size,),
                    w::text(name).height(icon_size).center()
                ]
                .spacing(structure.gap)
                .width(Fill)
                .padding(padding::vertical(structure.small_gap * 0.75).left(structure.small_gap))
            )
            .on_press(ChannelsMessage::ToggleCategory(room.room_id().to_owned()))
            .padding(0.0)
            .style(move |_, status| ButtonStyle {
                background: None,
                text_color: if status.active() {
                    theme.text.normal.into()
                } else {
                    theme.text.dim.into()
                },
                border: Border {
                    color: if status.active() {
                        theme.border.into()
                    } else {
                        Color::TRANSPARENT
                    },
                    width: structure.border_thickness,
                    radius: structure.inner_border_radius.into()
                },
                ..Default::default()
            })
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
                                    room,
                                    avatar_cache,
                                    presence_map,
                                    room_watchers,
                                    collapsed_categories,
                                    icon_size,
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
                                active,
                                avatar_cache,
                                presence_map,
                                room_watchers,
                                collapsed_categories,
                                icon_size,
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
        w::row![
            room.render_icon(icon_size, avatar_cache),
            w::text(name).height(icon_size).center()
        ]
        .spacing(structure.gap)
        .width(Fill)
        .padding(padding::vertical(structure.small_gap * 0.75).horizontal(structure.small_gap))
        .into()
    };

    w::button(content)
        .padding(0.0)
        .style(move |_, status| {
            let selected =
                is_active || matches!(status, button::Status::Hovered | button::Status::Pressed);
            ButtonStyle {
                background: is_active.then_some(theme.solid_hover_bg.into()),
                text_color: if selected && !is_space {
                    theme.text.normal.into()
                } else {
                    theme.text.dim.into()
                },
                border: Border {
                    color: if selected && !is_space {
                        theme.border.into()
                    } else {
                        Default::default()
                    },
                    width: structure.border_thickness,
                    radius: structure.inner_border_radius.into(),
                },
                ..Default::default()
            }
        })
        .on_press_maybe(if is_space {
            Some(ChannelsMessage::None)
        } else {
            (!is_active).then_some(ChannelsMessage::SetActiveRoom(room.clone()))
        })
        .into()
}
