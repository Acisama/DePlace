use std::collections::BTreeSet;

use deplace_core::state::{ActiveServer, PresenceMap};
use iced::widget::text::Alignment;
use macros::iced_cache;

use crate::{common::*, components::render_presence};

#[derive(Debug, Clone)]
pub enum ChannelsMessage {
    NeedsAvatar(OwnedMxcUri),
    SetActiveRoom(DePlaceRoom),
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

    membership_map: Receiver<MembershipMap>,
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

            membership_map: state.membership_map(),
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

        let channels: Arc<Vec<_>> = match &active_server {
            ActiveServer::Dms => self.room_watchers.dm_rooms(),
            ActiveServer::Server(server) => {
                Arc::new(self.room_watchers.get_children(server.room_id()))
            }
        };

        let membership_map = &self.membership_map.borrow();
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
                        active_room_id.clone(),
                        r,
                        &self.avatar_cache,
                        membership_map,
                        presence_map,
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
    room: &DePlaceRoom,
    avatar_cache: &AvatarCache,
    membership_map: &MembershipMap,
    presence_map: &PresenceMap,
    icon_size: f32,
) -> Element<'static, ChannelsMessage> {
    let is_active = active_room_id
        .as_ref()
        .is_some_and(|id| id == room.room_id());

    let (icon, name) = if room.is_dm()
        && let Some(other_member) = room.get_other_member(membership_map)
    {
        (
            render_presence(&other_member, presence_map, theme, icon_size, avatar_cache),
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
