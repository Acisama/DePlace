use std::collections::BTreeSet;

use crate::common::*;
use macros::iced_cache;
use matrix_sdk::room::RoomMember;

#[derive(Debug, Clone)]
pub enum ProfileCardMessage {
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for ProfileCardMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ProfileCardMessage::NeedsAvatar(uri)
    }
}

pub enum ProfileCardAction {
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Clone, Debug)]
pub struct ProfileCard {
    avatar_cache: AvatarCache,
    member: Option<RoomMember>,
}

impl ProfileCard {
    pub fn new(state: &AppState) -> Self {
        Self {
            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),

            member: None,
        }
    }

    pub fn set_member(&mut self, member: Option<RoomMember>) {
        self.member = member;
    }
}

impl IcedWidget<ProfileCardMessage, ProfileCardAction> for ProfileCard {
    fn update(&mut self, message: ProfileCardMessage) -> Option<ProfileCardAction> {
        match message {
            ProfileCardMessage::NeedsAvatar(uri) => {
                Some(ProfileCardAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, ProfileCardMessage> {
        let Some(member) = &self.member else {
            return w::container("No other member present").into();
        };

        let color = member.color();

        w::row![w::container("").style(move |_| ContainerStyle {
            background: Some(color.into()),
            ..Default::default()
        })]
        .height(Fill)
        .width(Fill)
        .into()
    }
}
