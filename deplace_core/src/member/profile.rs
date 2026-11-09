use std::sync::Arc;

use matrix_sdk::room::RoomMember;
use ruma::{
    OwnedMxcUri, OwnedUserId,
    events::room::{
        member::MembershipState,
        power_levels::UserPowerLevel::{self},
    },
};

use crate::cache::{ProfileAvatars, ThumbnailCache};

#[derive(Clone)]
#[non_exhaustive]
pub struct ProfileData {
    pub(crate) user_id: Arc<OwnedUserId>,
    pub(crate) avatar_url: Option<OwnedMxcUri>,

    pub(crate) displayname: Option<String>,
    pub(crate) display_string: String,
    pub(crate) initial: char,

    pub(crate) membership: MembershipState,
    pub(crate) power_level: UserPowerLevel,

    pub(crate) avatars: Option<ProfileAvatars>,
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

            avatars: None,
        }
    }
}

impl ProfileData {
    pub(crate) fn from_room_member(
        m: &RoomMember,
        cache: &ThumbnailCache,
        eagerly_load: bool,
    ) -> Self {
        let avatar_url = m.avatar_url().map(|u| u.to_owned());
        let avatars = avatar_url
            .clone()
            .map(|url| ProfileAvatars::get(cache, url));

        if eagerly_load && let Some(avatar) = &avatars {
            avatar.start_load_all();
        }

        let mut member = Self {
            user_id: Arc::new(m.user_id().to_owned()),
            membership: m.membership().clone(),
            power_level: m.power_level(),
            avatar_url,

            displayname: m.display_name().map(|n| n.to_owned()),
            display_string: String::new(),
            initial: '?',

            avatars,
        };

        member.calculate_strings();
        member
    }

    pub(crate) fn reload_avatars(&mut self, new_url: Option<OwnedMxcUri>, cache: &ThumbnailCache) {
        if let Some(url) = new_url.clone() {
            self.avatars = Some(ProfileAvatars::get(cache, url));
        }
        self.avatar_url = new_url;
    }

    pub(crate) fn calculate_strings(&mut self) {
        if let Some(name) = &self.displayname {
            self.display_string = name.to_owned();
        } else {
            self.display_string = self.user_id.to_string();
        }
        self.initial = self
            .display_string
            .to_uppercase()
            .chars()
            .next()
            .and_then(|c| c.to_uppercase().next())
            .unwrap_or('?');
    }
}
