use anyhow::{Error, Result};
use matrix_sdk::{
    Client, SqliteStoreConfig,
    config::RequestConfig,
    encryption::{BackupDownloadStrategy, EncryptionSettings},
    reqwest::Url,
    ruma::{DeviceId, UserId},
    search_index::SearchIndexStoreKind,
};

use crate::{
    APP_NAME, DEVICE_DISPLAY_NAME,
    keyring::{self, StoredSession, get_or_create_store_key},
    state::{AppState, UserDevice},
};

pub async fn test_server(server_name_or_url: String) -> Option<(Client, Url)> {
    tracing::trace!("Testing server: {server_name_or_url}");
    let client = Client::builder()
        .server_name_or_homeserver_url(&server_name_or_url)
        .request_config(RequestConfig::short_retry())
        .build()
        .await
        .ok()?;

    let homeserver = client.homeserver();

    tracing::debug!("Valid homeserver: {homeserver}");

    Some((client, homeserver))
}

pub enum LoginResult {
    Success(AppState),
    InvalidCredentials,
    Error(String),
    BackToDiscovery,
}

impl Default for LoginResult {
    fn default() -> Self {
        LoginResult::Error("Unknown error".to_string())
    }
}

pub async fn login(
    temp_client: &Client,
    username: String,
    password: String,
    recovery_key: String,
) -> LoginResult {
    if temp_client
        .matrix_auth()
        .login_username(&username, &password)
        .initial_device_display_name(DEVICE_DISPLAY_NAME)
        .send()
        .await
        .is_err()
    {
        return LoginResult::InvalidCredentials;
    }

    let url = temp_client.homeserver();

    tracing::debug!("Logged in with temporary client, fetching session info");

    let user_id = match temp_client.user_id() {
        Some(id) => id.to_owned(),
        None => return LoginResult::Error("Failed to get user ID".to_string()),
    };
    let device_id = match temp_client.device_id() {
        Some(id) => id.to_owned(),
        None => return LoginResult::Error("Failed to get device ID".to_string()),
    };

    let client = match matrix_client_builder(&user_id, &device_id, url).await {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("Failed to create login client: {e}");
            return LoginResult::Error(e.to_string());
        }
    };

    let Some(session) = temp_client.session() else {
        return LoginResult::Error("Failed to get session from temporary client".to_string());
    };

    if let Err(e) = client.restore_session(session).await {
        tracing::error!("Failed to restore session on login client: {:?}", e);
        return LoginResult::Error(e.to_string());
    }

    if let Err(e) = client
        .encryption()
        .recovery()
        .recover(recovery_key.as_str())
        .await
    {
        tracing::error!("Recovery failed: {:?}", e);
        return LoginResult::InvalidCredentials;
    }

    save_session(&client);

    LoginResult::Success(AppState::new(
        client,
        UserDevice {
            user_id: user_id.to_owned(),
            device_id: device_id.to_owned(),
        },
    ))
}

pub fn save_session(client: &Client) {
    let Some(user_id) = client.user_id() else {
        tracing::error!("Tried to save while not logged in");
        return;
    };
    let Some(device_id) = client.device_id() else {
        tracing::error!("device_id is not available");
        return;
    };
    let server_url = client.homeserver();

    let session = StoredSession {
        user_id: user_id.to_owned(),
        device_id: device_id.to_owned(),
        access_token: client.session().unwrap().access_token().to_string(),
        refresh_token: client
            .session()
            .unwrap()
            .get_refresh_token()
            .map(|t| t.to_string()),
        homeserver_url: server_url,
    };

    tokio::task::spawn_blocking(move || {
        if let Err(e) = keyring::save_session(&session) {
            tracing::error!("Failed to save session: {}", e);
        }
    });
}

pub async fn matrix_client_builder(
    user_id: &UserId,
    device_id: &DeviceId,
    server_url: Url,
) -> Result<Client> {
    let safe_user_id = user_id.to_string().replace(':', "_");

    let data_dir = dirs::data_dir()
        .ok_or(Error::msg("Failed to get data directory"))?
        .join(APP_NAME);
    let cache_dir = dirs::cache_dir()
        .ok_or(Error::msg("Failed to get cache directory"))?
        .join(APP_NAME);

    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&cache_dir)?;

    let name = format!("{}_{}", safe_user_id, device_id);

    let db_path = data_dir.join(format!("{name}.db"));
    let cache_path = cache_dir.join("sessions-cache").join(&name);
    let index_path = data_dir.join("sessions-index").join(&name);

    std::fs::create_dir_all(&index_path)?;
    std::fs::create_dir_all(&cache_path)?;

    let store_key = get_or_create_store_key(user_id.as_str()).await?;

    let sqlite_store_config = SqliteStoreConfig::new(db_path).key(Some(&store_key));

    let password = hex::encode(store_key);
    let new_client = Client::builder()
        .homeserver_url(
            Url::parse(server_url.as_str()).expect("Valid homeserverurl from other client"),
        )
        .request_config(RequestConfig::short_retry())
        .handle_refresh_tokens()
        .sqlite_store_with_config_and_cache_path(sqlite_store_config, Some(cache_path))
        .search_index_store(SearchIndexStoreKind::EncryptedDirectory(
            index_path, password,
        ))
        .with_encryption_settings(EncryptionSettings {
            backup_download_strategy: BackupDownloadStrategy::AfterDecryptionFailure,
            ..Default::default()
        })
        .build()
        .await?;

    Ok(new_client)
}
