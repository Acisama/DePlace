use std::collections::BTreeSet;

use macros::iced_cache;

use crate::common::*;

#[derive(Debug, Clone)]
pub enum MemberListMessage {
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for MemberListMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatar(uri)
    }
}

pub enum MemberListAction {
    NeedsMedia(NeedsMedia),
}

#[iced_cache(Debug, Clone)]
pub struct MemberList {
    state: AppState,

    membership_map: Receiver<MembershipMap>,
    avatar_cache: AvatarCache,
}

impl MemberList {
    pub fn new(state: &AppState) -> Self {
        Self {
            state: state.clone(),

            membership_map: state.membership_map(),
            avatar_cache: state.avatar_cache().clone(),

            avatar_states_for_hash: BTreeSet::new(),
        }
    }
}

impl IcedWidget<MemberListMessage, MemberListAction> for MemberList {
    fn update(&mut self, message: MemberListMessage) -> Option<MemberListAction> {
        match message {
            MemberListMessage::NeedsAvatar(uri) => {
                Some(MemberListAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, MemberListMessage> {
        w::text("Member list").into()
    }
}
