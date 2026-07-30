use std::collections::HashMap;

use matrix_sdk::{Client, ruma::OwnedRoomId};
use ruma::events::{GlobalAccountDataEventContent, StaticEventContent};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use matrix_sdk::ruma::events::macros::EventContent;

#[derive(Debug, Serialize, Clone, Default, Deserialize, EventContent)]
#[ruma_event(type = "com.deplace.breadcrumbs", kind = GlobalAccountData)]
pub struct BreadcrumbsContent {
    #[serde(default)]
    pub recent_rooms: Vec<OwnedRoomId>,
    #[serde(default)]
    pub last_space_ids: HashMap<OwnedRoomId, OwnedRoomId>,
    #[serde(default)]
    pub last_dm_id: Option<OwnedRoomId>,
    #[serde(default)]
    pub last_single_id: Option<OwnedRoomId>,
    #[serde(default)]
    pub dms_last: bool,
}

#[derive(Debug, Serialize, Clone, Default, Deserialize, EventContent)]
#[ruma_event(type = "com.deplace.server-order", kind = GlobalAccountData)]
pub struct ServerOrderContent {
    #[serde(default)]
    pub servers: Vec<OwnedRoomId>,
}

pub async fn get_account_data<
    T: StaticEventContent<IsPrefix = ruma::events::False>
        + GlobalAccountDataEventContent
        + Default
        + DeserializeOwned,
>(
    client: &Client,
) -> T {
    client
        .account()
        .account_data::<T>()
        .await
        .map_err(|e| tracing::error!("Failed to get server order: {}", e))
        .ok()
        .flatten()
        .and_then(|raw| {
            raw.deserialize()
                .map_err(|e| tracing::error!("Failed to deserialize account data: {}", e))
                .ok()
        })
        .unwrap_or_default()
}

pub async fn set_account_data<T: StaticEventContent + GlobalAccountDataEventContent + Serialize>(
    client: &Client,
    data: T,
) {
    client
        .account()
        .set_account_data(data)
        .await
        .map_err(|e| tracing::error!("Failed to set account data: {}", e))
        .ok();
}
