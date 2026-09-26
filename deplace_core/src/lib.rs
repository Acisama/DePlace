#![recursion_limit = "256"]
use colors::DePlaceColor;
use matrix_sdk::{
    Client, Room, SessionMeta, SessionTokens, authentication::matrix::MatrixSession,
    room::RoomMember,
};
use ruma::{OwnedMxcUri, RoomId, UserId, room_id, user_id};
use state::{ActiveServer, ImportantPaths};

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
pub mod keybinds;
pub mod matrix_api;
pub mod notifications;
pub mod profile;
pub mod rooms;
pub mod search;
pub mod settings;
pub mod state;

pub mod structure;
pub mod theme;

pub use rooms::{DePlaceRoom, RoomWatcherHashingConfig, RoomWatchers};

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

pub const ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets");
pub const SHADER_PATH: &str = formatcp!("{ASSET_DIR}/loading.wgsl");
pub const ICON_SVG: &str = formatcp!("{ASSET_DIR}/deplace_icon.svg");
pub const ICON_PNG: &str = formatcp!("{ASSET_DIR}/deplace_icon.png");

#[derive(Debug, Clone, Copy)]
pub enum PaginationDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone)]
pub enum RestoreResult {
    Success(Box<AppState>),
    NoSession,
    NeedsLogin(Client),
}

pub async fn try_restore(paths: ImportantPaths) -> RestoreResult {
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

    let (client, settings, keybinds) =
        match matrix_client_builder(&user_id, &device_id, session.homeserver_url, &paths).await {
            Ok(stuff) => stuff,
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
    let state = match AppState::new(client.clone(), device, settings, keybinds, paths).await {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Failed to create app state: {e}");
            return RestoreResult::NeedsLogin(client);
        }
    };
    spawn_room_sync(&client, &state);

    tracing::info!("Restored session for user_id: {user_id}, device_id: {device_id}");
    RestoreResult::Success(Box::new(state))
}

pub fn get_room_name_fallback(room: &Room, fallback: &str) -> String {
    room.cached_display_name()
        .map(|n| n.to_string())
        .unwrap_or(fallback.to_string())
}

/// Used for anything which has a name, static id and avatar
pub trait ProfileLike {
    type Id<'a>: AsRef<str>
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32;
    fn profile_name(&self) -> Option<String>;
    fn profile_avatar(&self) -> Option<OwnedMxcUri>;
    fn profile_id(&self) -> Self::Id<'_>;

    fn get_name(&self) -> String {
        self.profile_name()
            .unwrap_or(self.profile_id().as_ref().to_string())
    }

    fn get_avatar(&self) -> Option<OwnedMxcUri> {
        self.profile_avatar()
    }

    fn color(&self) -> DePlaceColor {
        self.profile_id().as_ref().into()
    }

    fn initial(&self) -> char {
        self.get_name().chars().next().unwrap_or('?')
    }
}

impl ProfileLike for RoomMember {
    type Id<'a>
        = &'a UserId
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32 {
        0.5
    }

    fn profile_name(&self) -> Option<String> {
        Some(self.name().to_string())
    }

    fn profile_avatar(&self) -> Option<OwnedMxcUri> {
        self.avatar_url().map(|u| u.to_owned())
    }

    fn profile_id(&self) -> &UserId {
        self.user_id()
    }
}

impl ProfileLike for Option<RoomMember> {
    type Id<'a>
        = &'a UserId
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32 {
        0.5
    }

    fn profile_name(&self) -> Option<String> {
        self.as_ref().and_then(|m| m.profile_name())
    }

    fn profile_avatar(&self) -> Option<OwnedMxcUri> {
        self.as_ref().and_then(|m| m.profile_avatar())
    }

    fn profile_id(&self) -> Self::Id<'_> {
        self.as_ref()
            .map(|m| m.profile_id())
            .unwrap_or(user_id!("@unknown:matrix.org"))
    }
}

impl ProfileLike for Room {
    type Id<'a>
        = &'a RoomId
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32 {
        0.25
    }

    fn profile_name(&self) -> Option<String> {
        self.cached_display_name().map(|n| n.to_string())
    }

    fn profile_avatar(&self) -> Option<OwnedMxcUri> {
        self.avatar_url()
    }

    fn profile_id(&self) -> Self::Id<'_> {
        self.room_id()
    }
}

impl ProfileLike for Option<Room> {
    type Id<'a>
        = &'a RoomId
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32 {
        0.25
    }

    fn profile_name(&self) -> Option<String> {
        self.as_ref().and_then(|r| r.profile_name())
    }

    fn profile_avatar(&self) -> Option<OwnedMxcUri> {
        self.as_ref().and_then(|r| r.avatar_url())
    }

    fn profile_id(&self) -> Self::Id<'_> {
        self.as_ref()
            .map(|r| r.profile_id())
            .unwrap_or(room_id!("!unknown:matrix.org"))
    }
}

impl ProfileLike for ActiveServer {
    type Id<'a>
        = &'a RoomId
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32 {
        0.25
    }

    fn profile_name(&self) -> Option<String> {
        match self {
            ActiveServer::Dms => Some("Direct Messages".into()),
            ActiveServer::Server(room) => room.profile_name(),
        }
    }

    fn profile_avatar(&self) -> Option<OwnedMxcUri> {
        self.as_server().and_then(|s| s.avatar_url())
    }

    fn profile_id(&self) -> Self::Id<'_> {
        self.as_server()
            .map(|s| s.room_id())
            .unwrap_or(room_id!("!unknown:matrix.org"))
    }
}

pub fn window_title(room: Option<DePlaceRoom>, server: ActiveServer) -> String {
    let room_name = room.as_ref().map(|r| r.get_name());
    let server_name = server.as_server().map(|s| s.get_name());

    match (room_name, server_name) {
        (Some(room_name), Some(server_name)) => format!("#{} | {}", room_name, server_name),
        (Some(room_name), None) => format!("@{} - {APP_HUMAN_NAME}", room_name),
        (None, Some(server_name)) => format!("{} - {APP_HUMAN_NAME}", server_name),
        (None, None) => APP_HUMAN_NAME.to_string(),
    }
}
