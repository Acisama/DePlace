use std::{cmp::Reverse, collections::BTreeSet};

use deplace_core::{
    get_other_member, matrix_api::sync::ParentToChildrenOrderStr, state::ActiveServer,
};

use crate::common::*;

#[derive(Debug, Clone)]
pub enum ChannelsMessage {
    NeedAvatar(OwnedMxcUri),
    SetActiveRoom(Room),
}

impl NeedsAvatarExt for ChannelsMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedAvatar(uri)
    }
}

pub enum ChannelsAction {
    SetActiveRoom(Room),
    FetchAvatar(OwnedMxcUri),
}

#[derive(Clone)]
pub struct ServerChannels {
    state: AppState,
    own_id: OwnedUserId,
    avatar_cache: AvatarCache,

    dm_rooms: Receiver<RoomMap>,

    membership_map: Receiver<MembershipMap>,

    parent_to_children: Receiver<ParentToChildrenOrderStr>,

    active_room: Receiver<Option<Room>>,
    active_server: Receiver<ActiveServer>,

    avatar_states_for_hash: BTreeSet<OwnedMxcUri>,
}

impl Hash for ServerChannels {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.state.room_version().hash(state);
        self.state.active_server_version().hash(state);
        self.state.active_room_version().hash(state);
        self.state.membership_version().hash(state);
        self.state.presence_version().hash(state);

        for uri in &self.avatar_states_for_hash {
            self.state
                .avatar_cache()
                .get(uri)
                .unwrap_or_default()
                .hash(state)
        }
    }
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

            own_id: state.own_id(),
            state: state.clone(),
        }
    }

    pub fn update(&mut self, msg: ChannelsMessage) -> ChannelsAction {
        match msg {
            ChannelsMessage::NeedAvatar(uri) => {
                self.avatar_states_for_hash.retain(|u| {
                    !matches!(
                        self.state.avatar_cache().get(u).unwrap_or_default(),
                        MediaState::Failed | MediaState::Loaded(_)
                    )
                });
                self.avatar_states_for_hash.insert(uri.clone());
                ChannelsAction::FetchAvatar(uri)
            }
            ChannelsMessage::SetActiveRoom(room) => ChannelsAction::SetActiveRoom(room),
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ChannelsMessage> {
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
                w::container(text(active_server.get_name()).size(structure.large_font_size)),
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
                        &self.own_id,
                    )
                }))
            ],
        )
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
    own_id: &UserId,
) -> Element<'static, ChannelsMessage> {
    let is_active = active_room_id
        .as_ref()
        .is_some_and(|id| id == room.room_id());

    let icon = if room.is_dm()
        && let Some(other_member) = get_other_member(own_id, membership_map, room.room_id())
    {
        other_member.render_icon(icon_size, avatar_cache)
    } else {
        room.render_icon(icon_size, avatar_cache)
    };

    let id = room.room_id().to_owned();

    w::button(w::row![icon])
        .padding(
            Padding::default()
                .horizontal(structure.small_gap)
                .vertical(structure.small_gap * 0.75),
        )
        .style(move |_, status| ButtonStyle {
            background: is_active.then_some(theme.solid_hover_bg.into()),
            border: Border {
                color: if is_active
                    || !matches!(
                        status,
                        button::Status::Hovered | button::Status::Active | button::Status::Pressed
                    ) {
                    theme.border
                } else {
                    Default::default()
                },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            ..Default::default()
        })
        .on_press_maybe((!is_active).then_some(ChannelsMessage::SetActiveRoom(room.clone())))
        .into()
}
