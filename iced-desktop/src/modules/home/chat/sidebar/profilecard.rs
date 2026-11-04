use crate::{
    common::*,
    components::{CopyUserIdExt, profile::render_banner_column},
};
use macros::iced_cache;

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
    Perform(Task<ProfileCardMessage>),
}

#[iced_cache(Clone, Debug)]
pub struct ProfileCard {
    member: Option<Member>,
}

impl ExtraHash for ProfileCard {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if let Some(member) = &self.member {
            member.hash_all(state);
        } else {
            0.hash(state);
        }
    }
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

        render_banner_column(member, theme, structure)
    }
}
