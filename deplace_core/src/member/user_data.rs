use chrono::Duration;
use matrix_sdk::{Client, event_handler::Ctx};
use ruma::{
    UserId,
    events::presence::{PresenceEvent, PresenceEventContent},
    presence::PresenceState,
};

use crate::{
    colors::DePlaceColor,
    helpers::DisplayString,
    matrix_api::account_data::{
        BannerColorContent, DePlaceAccountData, NameColorContent, SonicSignatureDataContent,
    },
    state::AppState,
};

use super::synth::SonicSignature;

#[derive(Clone, Debug, Hash)]
pub struct CustomData {
    pub(crate) sonic_signature: SonicSignature,
    pub(crate) name_color: DePlaceColor,
    pub(crate) banner_color: DePlaceColor,
}

#[derive(Clone, Debug, Hash, Copy, PartialEq)]
pub enum Presence {
    Online,
    Offline,
    Unavailable,
}

impl From<PresenceState> for Presence {
    fn from(state: PresenceState) -> Self {
        match state {
            PresenceState::Online => Presence::Online,
            PresenceState::Offline => Presence::Offline,
            PresenceState::Unavailable => Presence::Unavailable,
            _ => Presence::Offline,
        }
    }
}

impl DisplayString for Presence {
    fn display_string(&self) -> String {
        match self {
            Presence::Offline => "Offline".to_string(),
            Presence::Online => "Online".to_string(),
            Presence::Unavailable => "Idle".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct UserData {
    pub(crate) presence: Presence,
    pub(crate) status: Option<String>,
    pub(crate) is_active: Option<bool>,
    pub(crate) last_active_ago: Option<Duration>,
    pub(crate) custom_data: CustomData,
}

impl UserData {
    pub(crate) fn new(user_id: &UserId) -> Self {
        Self {
            presence: Presence::Offline,
            status: None,
            is_active: None,
            last_active_ago: None,
            custom_data: CustomData::new(user_id),
        }
    }

    pub(crate) fn update(&mut self, event: PresenceEvent) {
        self.presence = event.content.presence.into();
        self.status = event.content.status_msg.clone();
        self.is_active = Some(true);
        self.last_active_ago = event
            .content
            .last_active_ago
            .and_then(|u| chrono::Duration::try_milliseconds(u.into()));
    }

    pub(crate) fn update_from_content(&mut self, event: PresenceEventContent) {
        self.presence = event.presence.into();
        self.status = event.status_msg.clone();
        self.is_active = Some(true);
        self.last_active_ago = event
            .last_active_ago
            .and_then(|u| chrono::Duration::try_milliseconds(u.into()));
    }
}

impl CustomData {
    pub fn new(user_id: &UserId) -> Self {
        Self {
            sonic_signature: SonicSignature::from_string(user_id.as_str()),
            name_color: DePlaceColor::from(user_id.as_str()),
            banner_color: DePlaceColor::from(user_id.as_str()),
        }
    }

    pub async fn load(client: &Client, user_id: &UserId) -> Self {
        let sonic_signature = SonicSignatureDataContent::fetch(client)
            .await
            .signature
            .unwrap_or_else(|| SonicSignature::from_string(user_id.as_str()));
        let name_color = NameColorContent::fetch(client)
            .await
            .color
            .unwrap_or_else(|| DePlaceColor::from(user_id.as_str()));
        let banner_color = BannerColorContent::fetch(client)
            .await
            .color
            .unwrap_or_else(|| DePlaceColor::from(user_id.as_str()));

        Self {
            sonic_signature,
            name_color,
            banner_color,
        }
    }
}

pub async fn handle_presence_update(event: PresenceEvent, state: Ctx<AppState>) {
    let (_, cell) = state.profile_store_ref().user_cell(&event.sender);
    cell.change(|data| {
        data.update(event.clone());
    });
}
