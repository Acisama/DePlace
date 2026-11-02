use std::collections::BTreeSet;

use crate::{
    common::*,
    components::{CopyUserIdExt, profile::render_banner_column},
};
use deplace_core::state::PresenceMap;
use macros::iced_cache;
use matrix_sdk::room::RoomMember;

#[derive(Debug, Clone)]
pub enum ProfileCardMessage {
    CopyUserId(OwnedUserId),
    UserIdCopied,
}

impl CopyUserIdExt for ProfileCardMessage {
    fn copy_user_id(id: OwnedUserId) -> Self {
        ProfileCardMessage::CopyUserId(id)
    }
}

pub enum ProfileCardAction {
    NeedsMedia(NeedsMedia),
    Perform(Task<ProfileCardMessage>),
}

#[iced_cache(Clone, Debug)]
pub struct ProfileCard {
    #[hash]
    member: Option<Member>,
}

impl ProfileCard {
    pub fn new(member: Option<Member>) -> Self {
        Self { member }
    }

    pub fn set_member(&mut self, member: Option<Member>) {
        self.member = member;
    }
}

impl IcedWidget<ProfileCardMessage, ProfileCardAction> for ProfileCard {
    fn update(&mut self, message: ProfileCardMessage) -> Option<ProfileCardAction> {
        match message {
            ProfileCardMessage::UserIdCopied => None,
            ProfileCardMessage::CopyUserId(user_id) => Some(ProfileCardAction::Perform(
                iced::clipboard::write(user_id.to_string())
                    .map_err(|_| tracing::error!("Failed to copy to clipboard"))
                    .map(|_| ProfileCardMessage::UserIdCopied),
            )),
        }
    }

    fn view(&self) -> iced::Element<'static, ProfileCardMessage> {
        let Some(member) = &self.member else {
            return w::container("No other member present").boxed();
        };

        let theme = theme();
        let structure = structure();

        render_banner_column(&member, theme, structure)
    }
}
