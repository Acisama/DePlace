use std::collections::BTreeSet;

use iced::Alignment;
use macros::iced_cache;

use crate::{common::*, components::context_room_icon};

#[derive(Debug, Clone)]
pub enum HeaderMessage {
    TogglePins,
    ToggleSearch,
    ToggleList,
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for HeaderMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        HeaderMessage::NeedsAvatar(uri)
    }
}

pub enum HeaderAction {
    NeedsMedia(NeedsMedia),
    TogglePins,
    ToggleSearch,
    ToggleList,
}

#[iced_cache(Clone)]
pub struct Header {
    state: AppState,
    avatar_cache: AvatarCache,

    #[hash]
    room_id: OwnedRoomId,

    room_watchers: RoomWatchers,

    membership_map: Receiver<MembershipMap>,
}

impl Header {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        let id = room.room_id().to_owned();
        Self {
            avatar_cache: state.avatar_cache().clone(),
            membership_map: state.membership_map().clone(),

            room_id: id.clone(),

            room_watchers: state
                .room_watchers(hashing::hash_room_default(id.clone()))
                .clone(),
            state: state.clone(),

            avatar_states_for_hash: BTreeSet::new(),
        }
    }
}

impl IcedWidget<HeaderMessage, HeaderAction> for Header {
    fn update(&mut self, msg: HeaderMessage) -> Option<HeaderAction> {
        match msg {
            HeaderMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(HeaderAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
            HeaderMessage::TogglePins => Some(HeaderAction::TogglePins),
            HeaderMessage::ToggleSearch => Some(HeaderAction::ToggleSearch),
            HeaderMessage::ToggleList => Some(HeaderAction::ToggleList),
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, HeaderMessage> {
        let Some(room) = self.room_watchers.get_room(&self.room_id) else {
            return w::Space::new().into();
        };

        let icon_size = structure.header.icon_size;
        let (icon, name) = if room.is_dm()
            && let Some(other_member) = room.get_other_member(&self.membership_map.borrow())
        {
            (
                other_member
                    .clone()
                    .render_icon(icon_size, &self.avatar_cache),
                other_member.render_name(structure.font_size),
            )
        } else {
            (
                w::container(context_room_icon(&room, icon_size, &self.avatar_cache))
                    .style(move |_| ContainerStyle {
                        text_color: Some(theme.text.normal.into()),
                        ..Default::default()
                    })
                    .into(),
                w::container(room.render_name(structure.font_size))
                    .style(move |_| ContainerStyle {
                        text_color: Some(theme.text.normal.into()),
                        ..Default::default()
                    })
                    .into(),
            )
        };

        floating_tile(
            theme,
            structure,
            w::row![icon, name]
                .align_y(Alignment::Center)
                .spacing(structure.gap),
        )
        .width(Fill)
        .padding(structure.header.icon_padding())
        .height(structure.header.height)
        .into()
    }
}
