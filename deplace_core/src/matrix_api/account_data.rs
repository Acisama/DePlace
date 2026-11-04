use std::collections::HashMap;

use matrix_sdk::{Client, ruma::OwnedRoomId};
use ruma::events::{GlobalAccountDataEventContent, StaticEventContent};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use matrix_sdk::ruma::events::macros::EventContent;

use crate::{colors::DePlaceColor, member::synth::SonicSignature};

#[derive(Debug, Serialize, Clone, Default, Deserialize, EventContent)]
#[ruma_event(type = "com.deplace.sonic_signature", kind = GlobalAccountData)]
pub struct SonicSignatureDataContent {
    #[serde(default)]
    pub signature: Option<SonicSignature>,
}

#[derive(Debug, Serialize, Clone, Default, Deserialize, EventContent)]
#[ruma_event(type = "com.deplace.name_color", kind = GlobalAccountData)]
pub struct NameColorContent {
    #[serde(default)]
    pub color: Option<DePlaceColor>,
}

#[derive(Debug, Serialize, Clone, Default, Deserialize, EventContent)]
#[ruma_event(type = "com.deplace.banner_color", kind = GlobalAccountData)]
pub struct BannerColorContent {
    #[serde(default)]
    pub color: Option<DePlaceColor>,
}

#[derive(Debug, Serialize, Clone, Default, Deserialize, EventContent)]
#[ruma_event(type = "com.deplace.breadcrumbs", kind = GlobalAccountData)]
pub struct BreadcrumbsContent {
    #[serde(default)]
    pub(crate) recent_rooms: Vec<OwnedRoomId>,
    #[serde(default)]
    pub(crate) last_space_ids: HashMap<OwnedRoomId, OwnedRoomId>,
    #[serde(default)]
    pub(crate) last_dm_id: Option<OwnedRoomId>,
    #[serde(default)]
    pub(crate) last_single_id: Option<OwnedRoomId>,
    #[serde(default)]
    pub(crate) dms_last: bool,
}

impl BreadcrumbsContent {
    pub fn recent_rooms(&self) -> &Vec<OwnedRoomId> {
        &self.recent_rooms
    }
    pub fn last_space_ids(&self) -> &HashMap<OwnedRoomId, OwnedRoomId> {
        &self.last_space_ids
    }
    pub fn last_dm_id(&self) -> &Option<OwnedRoomId> {
        &self.last_dm_id
    }
}

#[derive(Debug, Serialize, Clone, Default, Deserialize, EventContent)]
#[ruma_event(type = "com.deplace.server_order", kind = GlobalAccountData)]
pub struct ServerOrderContent {
    #[serde(default)]
    pub servers: Vec<OwnedRoomId>,
}

pub trait DePlaceAccountData
where
    Self: StaticEventContent<IsPrefix = ruma::events::False>
        + GlobalAccountDataEventContent
        + Default
        + DeserializeOwned
        + Clone,
{
    fn fetch(client: &Client) -> impl Future<Output = Self>;
    fn set(&self, client: &Client) -> impl Future<Output = ()>;
}

impl<T> DePlaceAccountData for T
where
    T: StaticEventContent<IsPrefix = ruma::events::False>
        + GlobalAccountDataEventContent
        + Default
        + DeserializeOwned
        + Clone,
{
    async fn fetch(client: &Client) -> Self {
        tracing::debug!("Getting account data of type \"{}\"", T::TYPE);

        let res = client
            .account()
            .account_data::<T>()
            .await
            .map_err(|e| {
                tracing::error!("Failed to get account data for {}: {}", T::TYPE, e);
                e
            })
            .ok()
            .flatten();

        if let Some(raw) = res {
            raw.deserialize()
                .map_err(|e| {
                    tracing::error!("Failed to deserialize {}: {}", T::TYPE, e);
                    e
                })
                .unwrap_or_default()
        } else {
            T::default()
        }
    }

    async fn set(&self, client: &Client) {
        tracing::debug!("Setting acccount data of type \"{}\"", T::TYPE);
        client
            .account()
            .set_account_data(self.clone())
            .await
            .map_err(|e| tracing::error!("Failed to set account data: {}", e))
            .ok();
    }
}
