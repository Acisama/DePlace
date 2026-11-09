use crate::{Shared, cache::ProfileAvatars, colors::DePlaceColor, polyicon::PolyIcon};
use chrono::Duration;
use profile::ProfileData;
use rodio::buffer::SamplesBuffer;
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use synth::{SignatureEvent, SonicSignature};
use user_data::{Presence, UserData};

use ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};

pub mod profile;
pub mod room;
pub mod store;
pub mod synth;
pub mod user_data;

pub use store::ProfileStore;

#[derive(Clone)]
pub struct Member {
    pub(crate) user_id: Arc<OwnedUserId>,
    pub(crate) room_id: Arc<OwnedRoomId>,
    pub(crate) room_data: Arc<Shared<ProfileData>>,
    pub(crate) user_data: Arc<Shared<UserData>>,
}

impl std::fmt::Debug for Member {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Member")
            .field("user_id", &self.user_id)
            .field("room_id", &self.room_id)
            .finish()
    }
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
    }

    pub fn hash_all<H: Hasher>(&self, state: &mut H) {
        self.hash_selective(state, true, true, true);
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

    pub fn name_color(&self) -> DePlaceColor {
        self.user_data.read().custom_data.name_color
    }

    pub fn banner_color(&self) -> DePlaceColor {
        self.user_data.read().custom_data.banner_color
    }

    pub fn avatars(&self) -> Option<ProfileAvatars> {
        self.room_data.read().avatars.clone()
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

    pub fn calc_synth_sound(&self, event: SignatureEvent) -> SamplesBuffer<f32> {
        self.user_data
            .read()
            .custom_data
            .sonic_signature
            .render_samples(event)
    }

    pub fn polyicon(&self) -> PolyIcon {
        self.user_data.read().custom_data.polyicon
    }
}
