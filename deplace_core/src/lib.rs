#![recursion_limit = "256"]
use crate::state::MembershipMap;
use matrix_sdk::{
    Client, Room, SessionMeta, SessionTokens, authentication::matrix::MatrixSession,
    room::RoomMember,
};
use ruma::{RoomId, UserId};
use state::ActiveServer;

use crate::{
    keyring::init_keyring,
    matrix_api::{matrix_client_builder, sync::spawn_room_sync},
    state::{AppState, UserDevice},
};
use const_format::formatcp;

mod keyring;

pub mod colors;
pub mod formatting;
pub mod helpers;
pub mod matrix_api;
pub mod profile;
pub mod settings;
pub mod state;

pub const APP_HUMAN_NAME: &str = "DePlace";
pub const APP_NAME: &str = "deplace";
pub const APP_MATRIX_NAME: &str = formatcp!("com.{APP_NAME}");

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

pub use state::RoomMap;

#[derive(Debug, Clone)]
pub enum RestoreResult {
    Success(Box<AppState>),
    NoSession,
    NeedsLogin(Client),
}

pub async fn try_restore() -> RestoreResult {
    init_keyring();

    let session = match tokio::task::spawn_blocking(keyring::get_last_active_session).await {
        Ok(Ok(Some(session))) => session,
        Ok(Ok(None)) => {
            tracing::info!("No active session found");
            return RestoreResult::NoSession;
        }
        Ok(Err(error)) => {
            tracing::error!("Failed to get last active session: {:?}", error);
            return RestoreResult::NoSession;
        }
        Err(error) => {
            tracing::error!("Failed to get last active session: {:?}", error);
            return RestoreResult::NoSession;
        }
    };

    let user_id = session.user_id;
    let device_id = session.device_id;

    let (client, settings) =
        match matrix_client_builder(&user_id, &device_id, session.homeserver_url).await {
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

    settings.refresh().await;

    matrix_api::save_session(&client);

    let device = UserDevice {
        user_id: user_id.clone(),
        device_id: device_id.clone(),
    };
    let state = AppState::new(client.clone(), device, settings).await;
    spawn_room_sync(&client, &state);

    tracing::info!("Restored session for user_id: {user_id}, device_id: {device_id}");
    RestoreResult::Success(Box::new(state))
}

pub fn get_room_name_fallback(room: &Room, fallback: &str) -> String {
    room.cached_display_name()
        .map(|n| n.to_string())
        .unwrap_or(fallback.to_string())
}

pub fn get_dm_room_name(room: &Room, map: &MembershipMap, own_id: &UserId) -> String {
    if !room.is_dm() {
        return "Unknown Room".to_string();
    }

    let room_id = room.room_id();

    let other_member = get_other_member(own_id, map, room_id);
    other_member
        .map(|m| m.get_name())
        .unwrap_or("Unknown Room".to_string())
}

pub fn get_other_member(
    own_id: &UserId,
    map: &MembershipMap,
    room_id: &RoomId,
) -> Option<RoomMember> {
    let members = map.get(room_id).cloned().unwrap_or_default();
    members
        .iter()
        .find(|(id, _)| *id != own_id)
        .map(|(_, m)| m.clone())
}

pub trait NameExt {
    fn get_name(&self) -> String;
    fn initial(&self) -> char;
}

impl NameExt for RoomMember {
    fn get_name(&self) -> String {
        self.display_name()
            .map(|n| n.to_string())
            .unwrap_or(self.user_id().to_string())
    }

    fn initial(&self) -> char {
        self.get_name().chars().next().unwrap_or('?')
    }
}

impl NameExt for Option<&RoomMember> {
    fn get_name(&self) -> String {
        self.as_ref()
            .map(|m| m.get_name())
            .unwrap_or("Unknown".to_string())
    }

    fn initial(&self) -> char {
        self.as_ref().map(|m| m.initial()).unwrap_or('?')
    }
}

impl NameExt for Room {
    fn get_name(&self) -> String {
        self.cached_display_name()
            .map(|n| n.to_string())
            .unwrap_or("Unknown Room".to_string())
    }

    fn initial(&self) -> char {
        self.get_name().chars().next().unwrap_or('?')
    }
}

impl NameExt for Option<Room> {
    fn get_name(&self) -> String {
        self.as_ref()
            .map(|r| r.get_name())
            .unwrap_or("Unknown Room".to_string())
    }

    fn initial(&self) -> char {
        self.as_ref().map(|r| r.initial()).unwrap_or('?')
    }
}

impl NameExt for ActiveServer {
    fn get_name(&self) -> String {
        match self {
            ActiveServer::Dms => "Direct Messages".to_string(),
            ActiveServer::Server(server) => server.get_name(),
        }
    }

    fn initial(&self) -> char {
        match self {
            ActiveServer::Dms => 'D',
            ActiveServer::Server(server) => server.initial(),
        }
    }
}

pub fn window_title(room: Option<Room>, server: ActiveServer) -> String {
    let room_name = room.as_ref().map(|r| r.get_name());
    let server_name = server.as_server().map(|s| s.get_name());

    match (room_name, server_name) {
        (Some(room_name), Some(server_name)) => format!("#{} | {}", room_name, server_name),
        (Some(room_name), None) => format!("@{} - {APP_HUMAN_NAME}", room_name),
        (None, Some(server_name)) => format!("{} - {APP_HUMAN_NAME}", server_name),
        (None, None) => APP_HUMAN_NAME.to_string(),
    }
}
