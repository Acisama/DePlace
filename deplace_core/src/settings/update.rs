use anyhow::Result;
use matrix_sdk::Client;
use ruma::events::GlobalAccountDataEventType;
use serde::{Serialize, de::DeserializeOwned, de::IntoDeserializer};
use toml_edit::DocumentMut;

use crate::settings::{MatrixSettingField, SETTINGS_TABLE};

pub async fn set_field_cloud<T: Serialize + Clone + DeserializeOwned>(
    client: &Client,
    value: &MatrixSettingField<T>,
) -> Result<()> {
    let json = value.to_raw()?;

    client
        .account()
        .set_account_data_raw(GlobalAccountDataEventType::from(value.cloud_name), json)
        .await?;

    Ok(())
}

async fn get_field_cloud<T: Serialize + Clone + DeserializeOwned + Default>(
    client: Client,
    cloud_name: &str,
) -> Result<T> {
    match client
        .account()
        .fetch_account_data(GlobalAccountDataEventType::from(cloud_name))
        .await
    {
        Ok(data) => {
            let res = if let Some(raw) = data {
                raw.deserialize_as_unchecked::<T>()?
            } else {
                T::default()
            };
            Ok(res)
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

async fn get_field_local<T: Serialize + Clone + DeserializeOwned + Default>(
    document: &DocumentMut,
    local_name: &str,
) -> Result<T> {
    let table = document
        .get(SETTINGS_TABLE)
        .ok_or(anyhow::anyhow!("No settings table found"))?;

    let Some(item) = table.get(local_name) else {
        return Ok(T::default());
    };

    let value = item
        .clone()
        .into_value()
        .map_err(|item| anyhow::anyhow!("`{local_name}` is not a value: {item:?}"))?;

    let res = T::deserialize(value.into_deserializer())?;
    Ok(res)
}
