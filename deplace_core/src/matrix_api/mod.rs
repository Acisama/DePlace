use anyhow::{Error, Result};
use matrix_sdk::{
    Client, SqliteStoreConfig,
    config::{RequestConfig, SyncSettings},
    encryption::{BackupDownloadStrategy, EncryptionSettings},
    reqwest::Url,
    ruma::UserId,
    search_index::SearchIndexStoreKind,
};
use ruma::DeviceId;

pub mod account_data;
// mod matrixrtc;
mod members;
pub mod messages;
pub mod presence;
pub mod sync;

use crate::{
    APP_NAME, DEVICE_DISPLAY_NAME,
    keyring::{self, StoredSession, get_or_create_store_key},
    matrix_api::sync::spawn_room_sync,
    settings::{SETTINGS_FILE_NAME, Settings},
    state::{AppState, UserDevice},
};

pub async fn test_server(url: Url) -> Option<Client> {
    tracing::trace!("Testing server: {url}");

    match Client::builder()
        .server_name_or_homeserver_url(url.as_str())
        .request_config(RequestConfig::short_retry())
        .build()
        .await
    {
        Ok(client) => {
            tracing::debug!("Valid homeserver: {}", url);
            Some(client)
        }
        Err(_) => {
            tracing::trace!("Invalid homeserver: {url}");
            None
        }
    }
}

pub enum LoginResult {
    Success(AppState),
    InvalidCredentials,
    Error(String),
}

pub enum EncryptionUpgradeResult {
    Verified,
    Error(String),
}

impl Default for LoginResult {
    fn default() -> Self {
        LoginResult::Error("Unknown error".to_string())
    }
}

/// Upgrades the clients encryption using the recovery key.
pub async fn recover_client_encryption(
    state: AppState,
    recovery_key: String,
) -> EncryptionUpgradeResult {
    let Ok(_) = state
        .client()
        .encryption()
        .recovery()
        .recover(&recovery_key)
        .await
    else {
        tracing::error!("Recovery failed");
        return EncryptionUpgradeResult::Error("Recovery failed".to_string());
    };

    tracing::info!("Restored encryption");

    tracing::info!("Spawned room sync");

    state.settings().refresh().await;

    EncryptionUpgradeResult::Verified
}

pub enum LoginMethod<F, Fut>
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: Future<Output = matrix_sdk::Result<()>> + Send + 'static,
{
    Credentials {
        old_client: Client,
        username: String,
        password: String,
    },
    Sso {
        authenticated_client: Client,
        url_handler: F,
    },
}

/// Try to log into the homeserver of the provided client
///
/// This uses the users credentials to direcly log in with the homeserver. This
/// might not be supported and instead the user might need to log into their
/// account using SSO, see `login_sso`.
///
/// If successful, this will return `LoginResult::ValidCredentials(Client)` with a new client
/// that is logged into the account. This client will not have the encryption
/// keys and will have to get them either using the recovery key or from another
/// device that has them
pub async fn login<F, Fut>(method: LoginMethod<F, Fut>) -> LoginResult
where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: Future<Output = matrix_sdk::Result<()>> + Send + 'static,
{
    // let temp_client = match Client::new(url.clone()).await {
    //     Ok(c) => c,
    //     Err(e) => {
    //         tracing::error!("Failed to construct client: {e}");
    //         return LoginResult::Error("Failed to construct client".to_string());
    //     }
    // };

    // if temp_client
    //     .matrix_auth()
    //     .login_username(&username, &password)
    //     .initial_device_display_name(DEVICE_DISPLAY_NAME)
    //     .send()
    //     .await
    //     .is_err()
    // {
    //     return LoginResult::InvalidCredentials;
    // }

    let (url, temp_client) = match method {
        LoginMethod::Credentials {
            username,
            password,
            old_client,
        } => {
            let temp_client = match Client::new(old_client.homeserver().clone()).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to construct client: {e}");
                    return LoginResult::Error("Failed to construct client".to_string());
                }
            };
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
            (old_client.homeserver(), temp_client)
        }
        LoginMethod::Sso {
            authenticated_client,
            url_handler,
        } => {
            if let Err(e) = authenticated_client
                .matrix_auth()
                .login_sso(url_handler)
                .initial_device_display_name(DEVICE_DISPLAY_NAME)
                .await
            {
                tracing::error!("SSO login failed: {e}");
                return LoginResult::Error(e.to_string());
            }
            (authenticated_client.homeserver(), authenticated_client)
        }
    };

    tracing::debug!("Logged in with temporary client, fetching session info");

    let user_id = match temp_client.user_id() {
        Some(id) => id.to_owned(),
        None => return LoginResult::Error("Failed to get user ID".to_string()),
    };
    let device_id = match temp_client.device_id() {
        Some(id) => id.to_owned(),
        None => return LoginResult::Error("Failed to get device ID".to_string()),
    };

    let (client, settings) = match matrix_client_builder(&user_id, &device_id, url).await {
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

    if let Err(e) = client.sync_once(SyncSettings::default()).await {
        tracing::error!("Initial sync failed: {:?}", e);
        return LoginResult::Error(e.to_string());
    }
    settings.refresh().await;

    let device = UserDevice {
        user_id: user_id.to_owned(),
        device_id: device_id.to_owned(),
    };

    save_session(&client);

    let state = AppState::new(client.clone(), device.clone(), settings).await;
    spawn_room_sync(&client, &state);

    LoginResult::Success(state)
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

    let (access_token, refresh_token) = match client.session() {
        Some(s) => (
            s.access_token().to_string(),
            s.get_refresh_token().map(|t| t.to_string()),
        ),
        None => {
            tracing::warn!("Failed to save session: no session available");
            return;
        }
    };

    let session = StoredSession {
        user_id: user_id.to_owned(),
        device_id: device_id.to_owned(),
        access_token,
        refresh_token,
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
) -> Result<(Client, Settings)> {
    let safe_user_id = user_id.to_string().replace(':', "_");

    let data_dir = dirs::data_dir()
        .ok_or(Error::msg("Failed to get data directory"))?
        .join(APP_NAME);
    let cache_dir = dirs::cache_dir()
        .ok_or(Error::msg("Failed to get cache directory"))?
        .join(APP_NAME);
    let settings_dir = dirs::config_dir()
        .ok_or(Error::msg("Failed to get config directory"))?
        .join(APP_NAME);

    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&cache_dir)?;
    std::fs::create_dir_all(&settings_dir)?;

    let name = format!("{safe_user_id}_{device_id}");
    let db_path = data_dir.join(format!("{name}.db"));
    let cache_path = cache_dir.join("sessions-cache").join(&name);
    let index_path = data_dir.join("sessions-index").join(&name);

    std::fs::create_dir_all(&index_path)?;
    std::fs::create_dir_all(&cache_path)?;

    let Ok(store_key) = get_or_create_store_key(user_id.as_str()).await else {
        return Err(Error::msg("Failed to get or create store key"));
    };

    let sqlite_store_config = SqliteStoreConfig::new(db_path).key(Some(&store_key));

    let password = hex::encode(store_key);
    let Ok(new_client) = Client::builder()
        .homeserver_url(server_url)
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
        .await
    else {
        return Err(Error::msg("Failed to build client"));
    };

    let settings = Settings::new(settings_dir.join(SETTINGS_FILE_NAME), new_client.clone());

    Ok((new_client, settings))
}
