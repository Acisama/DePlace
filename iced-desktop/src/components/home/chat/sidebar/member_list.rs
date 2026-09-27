use std::collections::BTreeSet;

use deplace_core::state::PresenceMap;
use macros::iced_cache;
use matrix_sdk::ruma::{events::presence::PresenceEventContent, presence::PresenceState};

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

    room_id: OwnedRoomId,

    membership_map: Receiver<MembershipMap>,
    presence_map: Receiver<PresenceMap>,

    avatar_cache: AvatarCache,
}

impl MemberList {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        Self {
            state: state.clone(),

            room_id: room.room_id().to_owned(),

            membership_map: state.membership_map(),
            presence_map: state.presence_map(),

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
        let presence_map = self.presence_map.borrow();
        let (online_members, offline_members): (Vec<_>, Vec<_>) = self
            .membership_map
            .borrow()
            .get(&self.room_id)
            .cloned()
            .unwrap_or_default()
            .values()
            .map(|member| {
                (
                    presence_map
                        .get(member.user_id())
                        .cloned()
                        .unwrap_or(PresenceEventContent::new(PresenceState::Offline)),
                    member,
                )
            })
            .partition(|(presence, _)| !matches!(presence.presence, PresenceState::Offline));

        w::text("Member list").into()
    }
}
