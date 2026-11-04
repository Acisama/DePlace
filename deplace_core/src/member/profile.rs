use std::sync::Arc;

use matrix_sdk::room::RoomMember;
use ruma::{
    OwnedMxcUri, OwnedUserId,
    events::room::{
        member::MembershipState,
        power_levels::UserPowerLevel::{self},
    },
};

#[cfg(feature = "iced")]
use crate::Avatar;

#[derive(Clone, Debug)]
pub struct ProfileData {
    pub(crate) user_id: Arc<OwnedUserId>,
    pub(crate) avatar_url: Option<OwnedMxcUri>,

    pub(crate) displayname: Option<String>,
    pub(crate) display_string: String,
    pub(crate) initial: char,

    pub(crate) membership: MembershipState,
    pub(crate) power_level: UserPowerLevel,
    #[cfg(feature = "iced")]
    pub(crate) avatar_handle: Avatar,
}

impl ProfileData {
    pub fn new(user_id: Arc<OwnedUserId>) -> Self {
        Self {
            display_string: user_id.to_string(),
            user_id,
            avatar_url: None,

            displayname: None,
            initial: '?',

            membership: MembershipState::Join,
            power_level: UserPowerLevel::Infinite,
            #[cfg(feature = "iced")]
            avatar_handle: Avatar::NoAvatar,
        }
    }
}

impl From<&RoomMember> for ProfileData {
    fn from(m: &RoomMember) -> Self {
        let mut member = Self {
            user_id: Arc::new(m.user_id().to_owned()),
            membership: m.membership().clone(),
            power_level: m.power_level(),
            avatar_url: m.avatar_url().map(|u| u.to_owned()),

            displayname: m.display_name().map(|n| n.to_owned()),
            display_string: String::new(),
            initial: '?',

            #[cfg(feature = "iced")]
            avatar_handle: Avatar::NoAvatar,
        };

        member.calculate_strings();
        member
    }
}

impl ProfileData {
    pub(crate) fn avatar_url(&self) -> Option<&OwnedMxcUri> {
        self.avatar_url.as_ref()
    }

    pub(crate) fn calculate_strings(&mut self) {
        if let Some(name) = &self.displayname {
            self.display_string = name.to_owned();
        } else {
            self.display_string = self.user_id.to_string();
        }
        self.initial = self.display_string.chars().next().unwrap_or('?');
    }
}
