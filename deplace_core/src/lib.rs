use matrix_sdk::{Client, SessionMeta, SessionTokens, authentication::matrix::MatrixSession};

use crate::{
    keyring::init_keyring,
    matrix_api::matrix_client_builder,
    state::{AppState, UserDevice},
};
use const_format::formatcp;

mod keyring;
pub mod matrix_api;
pub mod state;

pub const APP_HUMAN_NAME: &str = "DePlace";
const APP_NAME: &str = "deplace";

#[cfg(target_os = "linux")]
const PLATFORM: &str = "linux";
#[cfg(target_os = "windows")]
const PLATFORM: &str = "windows";
#[cfg(target_os = "macos")]
const PLATFORM: &str = "macos";
#[cfg(target_os = "android")]
const PLATFORM: &str = "android";
#[cfg(target_os = "ios")]
const PLATFORM: &str = "ios";

const DEVICE_DISPLAY_NAME: &str = formatcp!("DePlace on {PLATFORM}");

pub enum RestoreResult {
    Success(AppState),
    NoSession,
    NeedsLogin(Client),
}

pub async fn try_restore() -> RestoreResult {
    init_keyring();

    let session = match tokio::task::spawn_blocking(keyring::get_last_active_session)
        .await
        .expect("Keyring blocking task panicked")
    {
        Ok(Some(session)) => session,
        Ok(None) => {
            tracing::info!("No active session found");
            return RestoreResult::NoSession;
        }
        Err(error) => {
            tracing::error!("Failed to get last active session: {:?}", error);
            return RestoreResult::NoSession;
        }
    };

    let user_id = session.user_id;
    let device_id = session.device_id;

    let client = match matrix_client_builder(&user_id, &device_id, session.homeserver_url).await {
        Ok(client) => client,
        Err(error) => {
            tracing::error!("Failed to build matrix client: {:?}", error);
            return RestoreResult::NoSession;
        }
    };

    if let Err(error) = client
        .restore_session(MatrixSession {
            meta: SessionMeta {
                user_id: user_id.clone(),
                device_id: device_id.clone(),
            },
            tokens: SessionTokens {
                access_token: session.access_token,
                refresh_token: session.refresh_token,
            },
        })
        .await
    {
        tracing::error!("Failed to restore session on client: {:?}", error);
        return RestoreResult::NoSession;
    }

    matrix_api::save_session(&client);

    tracing::info!("Restored session for user_id: {user_id}, device_id: {device_id}");
    RestoreResult::Success(AppState::new(client, UserDevice { user_id, device_id }))
}
