use std::{cmp::Reverse, collections::BTreeSet};

use deplace_core::{matrix_api::sync::ParentToChildrenOrderStr, state::ActiveServer};
use iced::widget::text::Alignment;
use macros::iced_cache;

use crate::common::*;

#[derive(Debug, Clone)]
pub enum ChannelsMessage {
    NeedsAvatar(OwnedMxcUri),
    SetActiveRoom(Room),
}

impl NeedsAvatarExt for ChannelsMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ChannelsMessage::NeedsAvatar(uri)
    }
}

pub enum ChannelsAction {
    SetActiveRoom(Room),
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone)]
pub struct ServerChannels {
    state: AppState,
    avatar_cache: AvatarCache,

    dm_rooms: Receiver<RoomMap>,

    membership_map: Receiver<MembershipMap>,

    parent_to_children: Receiver<ParentToChildrenOrderStr>,

    active_room: Receiver<Option<Room>>,
    active_server: Receiver<ActiveServer>,
}

impl ServerChannels {
    pub fn new(state: &AppState) -> Self {
        Self {
            avatar_cache: state.avatar_cache().clone(),
            dm_rooms: state.dm_rooms(),

            membership_map: state.membership_map(),
            parent_to_children: state.parent_to_children(),

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
            ChannelsMessage::SetActiveRoom(room) => Some(ChannelsAction::SetActiveRoom(room)),
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ChannelsMessage> {
        let active_server = self.active_server.borrow().clone();
        let active_room_id = self
            .active_room
            .borrow()
            .as_ref()
            .map(|r| r.room_id().to_owned());

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
                        active_room_id.clone(),
                        r,
                        &self.avatar_cache,
                        &self.membership_map.borrow(),
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
    active_room_id: Option<OwnedRoomId>,
    room: &Room,
    avatar_cache: &AvatarCache,
    membership_map: &MembershipMap,
    icon_size: f32,
) -> Element<'static, ChannelsMessage> {
    let is_active = active_room_id
        .as_ref()
        .is_some_and(|id| id == room.room_id());

    let (icon, name) = if room.is_dm()
        && let Some(other_member) = room.get_other_member(membership_map)
    {
        (
            other_member.render_icon(icon_size, avatar_cache),
            other_member.get_name(),
        )
    } else {
        (
            context_room_icon(room, icon_size, avatar_cache),
            room.get_name(),
        )
    };

    w::button(
        w::row![icon, w::text(name).height(icon_size).center()]
            .spacing(structure.gap)
            .width(Fill),
    )
    .padding(padding::horizontal(structure.small_gap).vertical(structure.small_gap * 0.75))
    .style(move |_, status| {
        let selected =
            is_active || matches!(status, button::Status::Hovered | button::Status::Pressed);
        ButtonStyle {
            background: is_active.then_some(theme.solid_hover_bg.into()),
            text_color: if selected {
                theme.text.normal.into()
            } else {
                theme.text.dim.into()
            },
            border: Border {
                color: if selected {
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
    .on_press_maybe((!is_active).then_some(ChannelsMessage::SetActiveRoom(room.clone())))
    .into()
}
