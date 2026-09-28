use std::collections::BTreeSet;

use crate::{
    common::*,
    components::{CopyUserIdExt, render_banner_column},
};
use deplace_core::state::PresenceMap;
use macros::iced_cache;
use matrix_sdk::room::RoomMember;

#[derive(Debug, Clone)]
pub enum ProfileCardMessage {
    NeedsAvatar(OwnedMxcUri),
    CopyUserId(OwnedUserId),
    UserIdCopied,
}

impl NeedsAvatarExt for ProfileCardMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        ProfileCardMessage::NeedsAvatar(uri)
    }
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
    state: AppState,

    avatar_cache: AvatarCache,
    presence_map: Receiver<PresenceMap>,

    member: Option<RoomMember>,
}

impl ExtraHash for ProfileCard {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.member
            .as_ref()
            .map(|m| (m.get_avatar(), m.get_name()))
            .hash(state);
    }
}

impl ProfileCard {
    pub fn new(state: &AppState) -> Self {
        Self {
            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),

            presence_map: state.presence_map().clone(),
            state: state.clone(),

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
            ProfileCardMessage::UserIdCopied => None,
            ProfileCardMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(ProfileCardAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
            ProfileCardMessage::CopyUserId(user_id) => Some(ProfileCardAction::Perform(
                iced::clipboard::write(user_id.to_string())
                    .map_err(|_| tracing::error!("Failed to copy to clipboard"))
                    .map(|_| ProfileCardMessage::UserIdCopied),
            )),
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

        render_banner_column(
            member,
            &self.presence_map.borrow(),
            &self.avatar_cache,
            theme,
            structure,
        )
    }
}
