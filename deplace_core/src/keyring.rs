use crate::APP_NAME;
use anyhow::{Error, Result};

use keyring_core::Entry;

use matrix_sdk::{
    reqwest::Url,
    ruma::{OwnedDeviceId, OwnedUserId},
};
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

const LAST_USER_KEY: &str = "__last_active_user__";

/// Represents a stored session for a user, containing all necessary information to restore the session.
/// This struct is serialized and stored securely in the system keyring.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StoredSession {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,

    pub access_token: String,
    pub refresh_token: Option<String>,

    pub homeserver_url: Url,
}

pub fn init_keyring() {
    #[cfg(target_os = "linux")]
    keyring_core::set_default_store(
        zbus_secret_service_keyring_store::Store::new().expect("Failed to init Linux keyring"),
    );

    #[cfg(target_os = "android")]
    keyring_core::set_default_store(
        android_native_keyring_store::Store::new().expect("Failed to init Android keyring"),
    );

    #[cfg(target_os = "windows")]
    keyring_core::set_default_store(
        windows_native_keyring_store::Store::new().expect("Failed to init Windows keyring"),
    );

    #[cfg(target_os = "macos")]
    keyring_core::set_default_store(
        apple_native_keyring_store::keychain::Store::new().expect("Failed to init macOS keyring"),
    );
}

/// Retrieves the last active session from the keyring, if it exists, and returns it as a `StoredSession` struct.
pub fn get_last_active_session() -> Result<Option<StoredSession>> {
    let entry = Entry::new(APP_NAME, LAST_USER_KEY)?;

    match entry.get_password() {
        Ok(user_id) => get_session(user_id),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Retrieves the session for a specific user ID from the keyring, if it exists, and returns it as a `StoredSession` struct.
pub fn get_session(user_id: String) -> Result<Option<StoredSession>> {
    let entry_key = format!("{}:session", user_id);
    let entry = Entry::new(APP_NAME, &entry_key)?;

    match entry.get_password() {
        Ok(session_json) => {
            let session: StoredSession = serde_json::from_str(&session_json)?;
            Ok(Some(session))
        }
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Saves the provided `StoredSession` struct securely in the system keyring, associating it with the user ID and marking it as the last active session.
pub fn save_session(session: &StoredSession) -> Result<()> {
    let entry_key = format!("{}:session", session.user_id);
    let entry = Entry::new(APP_NAME, &entry_key)?;

    let session_json = serde_json::to_string(session)?;

    entry.set_password(&session_json)?;

    let last_user_entry = Entry::new(APP_NAME, LAST_USER_KEY)?;
    last_user_entry.set_password(session.user_id.as_ref())?;

    Ok(())
}

/// Retrieves the existing store encryption key for the given user ID from the keyring, or
/// generates a new random one using the `getrandom` crate if none exists.
///
/// The key is already a uniformly random 256-bit secret (not a human-memorized password), so
/// it's passed to the sqlite store as a raw key rather than a passphrase, which skips a
/// pointless (and slow) PBKDF2 stretch on every store open.
pub async fn get_or_create_store_key(user_id: &str) -> Result<[u8; 32]> {
    let user_id = user_id.to_string();
    spawn_blocking(move || get_or_create_store_key_blocking(&user_id)).await?
}

fn get_or_create_store_key_blocking(user_id: &str) -> Result<[u8; 32]> {
    let entry = Entry::new(APP_NAME, &format!("passphrase:{}", user_id))?;

    let hex_key = match entry.get_password() {
        Ok(passphrase) => passphrase,
        Err(keyring_core::Error::NoEntry) => {
            tracing::info!(
                "No existing store key found for user {}, generating a new one",
                user_id
            );

            let mut key_bytes = [0u8; 32];

            getrandom::fill(&mut key_bytes)?;

            let new_passphrase = hex::encode(key_bytes);

            entry.set_password(&new_passphrase)?;

            tracing::info!("Generated and stored new store key for user {}", user_id);

            new_passphrase
        }
        Err(e) => return Err(e.into()),
    };

    let key_bytes = hex::decode(&hex_key)?;

    key_bytes
        .try_into()
        .map_err(|_| Error::msg("Stored key has unexpected length"))
}
