use crate::{Shared, colors::DePlaceColor};
use chrono::Duration;
use profile::ProfileData;
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use synth::SonicSignature;
use user_data::{Presence, UserData};

#[cfg(feature = "iced")]
use crate::Avatar;
use ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};

pub mod profile;
pub mod room;
pub mod store;
pub mod synth;
pub mod user_data;

pub use store::ProfileStore;

#[derive(Clone, Debug)]
pub struct Member {
    pub(crate) user_id: Arc<OwnedUserId>,
    pub(crate) room_id: Arc<OwnedRoomId>,
    pub(crate) room_data: Arc<Shared<ProfileData>>,
    pub(crate) user_data: Arc<Shared<UserData>>,
}

impl PartialEq for Member {
    fn eq(&self, other: &Self) -> bool {
        self.user_id == other.user_id && self.room_id == other.room_id
    }
}

impl Eq for Member {}

impl Hash for Member {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.user_id.hash(state);
        self.room_id.hash(state);
    }
}

impl Member {
    pub fn hash_selective<H: Hasher>(
        &self,
        state: &mut H,
        presence: bool,
        custom_data: bool,
        hash_name: bool,
        hash_avatar: bool,
    ) {
        self.user_id.hash(state);
        self.room_id.hash(state);

        if presence {
            self.user_data.data.load().presence.hash(state);
        }
        if custom_data {
            self.user_data.data.load().custom_data.hash(state);
        }
        if hash_name {
            self.room_data.data.load().display_string.hash(state);
        }
        if hash_avatar {
            self.room_data.data.load().avatar_url.hash(state);
        }
    }

    pub fn hash_all<H: Hasher>(&self, state: &mut H) {
        self.hash_selective(state, true, true, true, true);
    }

    pub(crate) fn update_room_profile(&self, f: impl Fn(&mut ProfileData)) {
        self.room_data.change(f);
    }

    pub(crate) fn raw_display_name_lowercase(&self) -> Option<String> {
        self.room_data
            .read()
            .displayname
            .clone()
            .map(|s| s.to_lowercase())
    }

    pub fn get_displayname(&self) -> String {
        self.room_data.read().display_string.clone()
    }

    pub fn get_initial(&self) -> char {
        self.room_data.read().initial
    }

    #[cfg(feature = "iced")]
    pub fn avatar_handle(&self) -> Avatar {
        self.room_data.read().avatar_handle.clone()
    }

    pub fn name_color(&self) -> DePlaceColor {
        self.user_data.read().custom_data.name_color
    }

    pub fn banner_color(&self) -> DePlaceColor {
        self.user_data.read().custom_data.banner_color
    }

    pub fn sonic_signature(&self) -> SonicSignature {
        self.user_data.read().custom_data.sonic_signature.clone()
    }

    pub fn user_id(&self) -> &UserId {
        &self.user_id
    }

    pub fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    pub fn presence(&self) -> Presence {
        self.user_data.read().presence
    }

    pub fn status(&self) -> Option<String> {
        self.user_data.read().status.clone()
    }

    pub fn is_active(&self) -> Option<bool> {
        self.user_data.read().is_active
    }

    pub fn last_active_ago(&self) -> Option<Duration> {
        self.user_data.read().last_active_ago
    }
}
