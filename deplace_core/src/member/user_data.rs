use chrono::Duration;
use ruma::{UserId, presence::PresenceState};

use crate::colors::DePlaceColor;

use super::synth::SonicSignature;

#[derive(Clone, Debug)]
pub struct CustomData {
    pub(super) sonic_signature: SonicSignature,
    pub(super) name_color: DePlaceColor,
    pub(super) banner_color: DePlaceColor,
}

#[derive(Clone, Debug)]
pub struct UserData {
    pub(super) presence: PresenceState,
    pub(super) status: Option<String>,
    pub(super) is_active: bool,
    pub(super) last_active_ago: Option<Duration>,
    pub(super) custom_data: CustomData,
}

impl UserData {
    pub fn new(user_id: &UserId) -> Self {
        Self {
            presence: PresenceState::Offline,
            status: None,
            is_active: false,
            last_active_ago: None,
            custom_data: CustomData {
                sonic_signature: SonicSignature::from_string(user_id.as_str()),
                name_color: DePlaceColor::from(user_id.as_str()),
                banner_color: DePlaceColor::from(user_id.as_str()),
            },
        }
    }
}
