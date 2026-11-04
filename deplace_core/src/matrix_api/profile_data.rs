use matrix_sdk::{Client, ruma::UserId};
use ruma::api::client::profile::{get_profile_field, set_profile_field};
use ruma::api::error::ErrorKind;
use ruma::profile::{ProfileFieldValue, StaticProfileField};

use crate::{colors::DePlaceColor, member::synth::SonicSignature};

#[derive(Debug, Clone, Copy)]
pub struct SonicSignatureField;

impl StaticProfileField for SonicSignatureField {
    type Value = SonicSignature;
    const NAME: &str = "com.deplace.sonic_signature";
}

#[derive(Debug, Clone, Copy)]
pub struct NameColorField;

impl StaticProfileField for NameColorField {
    type Value = DePlaceColor;
    const NAME: &str = "com.deplace.name_color";
}

#[derive(Debug, Clone, Copy)]
pub struct BannerColorField;

impl StaticProfileField for BannerColorField {
    type Value = DePlaceColor;
    const NAME: &str = "com.deplace.banner_color";
}

pub trait DePlaceProfileField
where
    Self: StaticProfileField,
{
    fn fetch(client: &Client, user_id: &UserId) -> impl Future<Output = Option<Self::Value>>;
    fn set(client: &Client, value: Self::Value) -> impl Future<Output = ()>;
}

impl<T> DePlaceProfileField for T
where
    T: StaticProfileField,
{
    async fn fetch(client: &Client, user_id: &UserId) -> Option<Self::Value> {
        let request = get_profile_field::v3::Request::new(user_id.to_owned(), Self::NAME.into());

        let response = match client.send(request).await {
            Ok(response) => response,
            // `NotFound`/`Unrecognized` just mean the field (or the endpoint, on servers
            // without extended profile support) isn't there - expected, not an error.
            Err(e)
                if matches!(
                    e.client_api_error_kind(),
                    Some(ErrorKind::NotFound | ErrorKind::Unrecognized)
                ) =>
            {
                return None;
            }
            Err(e) => {
                tracing::error!(
                    "Failed to get profile field {} for {user_id}: {e}",
                    Self::NAME
                );
                return None;
            }
        };

        let json = response.value?.value().into_owned();

        serde_json::from_value(json)
            .map_err(|e| tracing::error!("Failed to decode profile field {}: {e}", Self::NAME))
            .ok()
    }

    async fn set(client: &Client, value: Self::Value) {
        let Some(user_id) = client.user_id().map(|id| id.to_owned()) else {
            return;
        };

        let Ok(json) = serde_json::to_value(&value) else {
            tracing::error!("Failed to serialize profile field {}", Self::NAME);
            return;
        };

        let Ok(field_value) = ProfileFieldValue::new(Self::NAME, json) else {
            tracing::error!("Failed to encode profile field {}", Self::NAME);
            return;
        };

        tracing::debug!("Setting profile field \"{}\"", Self::NAME);

        client
            .send(set_profile_field::v3::Request::new(user_id, field_value))
            .await
            .map_err(|e| tracing::error!("Failed to set profile field {}: {e}", Self::NAME))
            .ok();
    }
}
