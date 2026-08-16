use anyhow::Result;
use matrix_sdk::Client;
use ruma::events::GlobalAccountDataEventType;
use serde::{Serialize, de::DeserializeOwned, de::IntoDeserializer};
use toml_edit::Table;

use crate::settings::{CloudSetting, MatrixSettingField};

pub async fn set_field_cloud<T: Serialize + Clone + DeserializeOwned + Default>(
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

pub async fn get_field_cloud<T: 'static + Serialize + Clone + DeserializeOwned>(
    client: Client,
    cloud_name: &str,
) -> Result<Option<CloudSetting<T>>> {
    let data = client
        .account()
        .fetch_account_data(GlobalAccountDataEventType::from(cloud_name))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    let Some(raw) = data else {
        return Ok(None);
    };

    Ok(Some(raw.deserialize_as_unchecked::<CloudSetting<T>>()?))
}

pub fn get_field_local<T: Serialize + Clone + DeserializeOwned + Default>(
    table: &Table,
    local_name: &str,
) -> Result<T> {
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
