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

pub enum HeaderAction {
    NeedsMedia(NeedsMedia),
    Run(Task<()>),
}

#[iced_cache(Clone)]
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
    pub fn new(state: &AppState, room: &Room) -> Self {
        Self {
            avatar_cache: state.avatar_cache().clone(),
            membership_map: state.membership_map().clone(),
            room: room.clone(),
            state: state.clone(),

            avatar_states_for_hash: BTreeSet::new(),
        }
    }

    pub fn load_media(&mut self, media: &MediaLoaded) {
        if let MediaLoaded::Avatar { uri } = media {
            self.avatar_states_for_hash.remove(uri);
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
            HeaderMessage::TogglePins => todo!(),
            HeaderMessage::ToggleSearch => todo!(),
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, HeaderMessage> {
        let room = &self.room;

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
                w::container(context_room_icon(room, icon_size, &self.avatar_cache))
                    .style(move |_| ContainerStyle {
                        text_color: Some(theme.text.normal),
                        ..Default::default()
                    })
                    .into(),
                w::container(room.render_name(structure.font_size))
                    .style(move |_| ContainerStyle {
                        text_color: Some(theme.text.normal),
                        ..Default::default()
                    })
                    .into(),
            )
        };

        floating_tile(theme, structure, w::row![icon, name].spacing(structure.gap))
            .width(Fill)
            .padding(structure.header.icon_padding())
            .height(structure.header.height)
            .into()
    }
}
