use std::collections::BTreeSet;

use macros::iced_cache;

use crate::{common::*, components::context_room_icon};

#[derive(Debug, Clone)]
pub enum HeaderMessage {
    TogglePins,
    ToggleSearch,
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for HeaderMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        HeaderMessage::NeedsAvatar(uri)
    }
}

#[iced_cache]
pub struct Header {
    state: AppState,
    avatar_cache: AvatarCache,

    membership_map: Receiver<MembershipMap>,

    room: Room,
}

impl ExtraHash for Header {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.room.room_id().hash(state);
    }
}

impl Header {
    pub fn new(state: &AppState, room: Room) -> Self {
        Self {
            avatar_cache: state.avatar_cache().clone(),
            membership_map: state.membership_map().clone(),
            room,
            state: state.clone(),

            avatar_states_for_hash: BTreeSet::new(),
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, HeaderMessage> {
        let room = &self.room;

        let icon_size = structure.header.icon_size;
        let icon = if room.is_dm()
            && let Some(other_member) = room.get_other_member(&self.membership_map.borrow())
        {
            other_member.render_icon(icon_size, &self.avatar_cache)
        } else {
            context_room_icon(room, icon_size, &self.avatar_cache)
        };

        floating_tile(theme, structure, w::row![icon, w::text(room.get_name())])
            .width(Fill)
            .padding(structure.header.icon_padding())
            .height(structure.header.height)
            .into()
    }
}
