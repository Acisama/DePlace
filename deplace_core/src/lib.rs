#![recursion_limit = "256"]

use std::sync::Arc;

use colors::DePlaceColor;
use polyicon::PolyIcon;
use room::call::CallMember;
use ruma::{OwnedRoomId, OwnedUserId, uint};

use matrix_sdk::{
    Client, Room, SessionMeta, SessionTokens,
    authentication::matrix::MatrixSession,
    media::{MediaFormat, MediaThumbnailSettings},
};
use state::{ActiveServer, ImportantPaths};

use crate::{
    keyring::init_keyring,
    matrix_api::{matrix_client_builder, sync::spawn_sync_runners},
    state::{AppState, UserDevice},
};
use const_format::formatcp;

mod keyring;

pub mod audio;
pub mod cache;
pub mod call;
pub mod colors;
pub mod formatting;
pub mod helpers;
pub mod keybinds;
pub mod matrix_api;
pub mod member;
pub mod notifications;
pub mod polyicon;
pub mod rich_text;
pub mod room;
pub mod search;
pub mod settings;
mod shared;
pub mod state;
pub use shared::Shared;

pub mod structure;
pub mod theme;

#[cfg(feature = "iced")]
pub use crate::matrix_api::avatar::Avatar;
pub use member::Member;
pub use room::DePlaceRoom;

pub const APP_HUMAN_NAME: &str = "DePlace";
pub const APP_NAME: &str = "deplace";
pub const APP_MATRIX_NAME: &str = formatcp!("com.{APP_NAME}");
pub const SOCKET_NAME: &str = "deplace_app.sock";

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

    let device = Arc::new(UserDevice {
        user_id: user_id.clone(),
        device_id: device_id.clone(),
    });
    let state = match AppState::new(client.clone(), device.clone(), settings, keybinds, paths).await
    {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Failed to create app state: {e}");
            return RestoreResult::NeedsLogin(client);
        }
    };
    spawn_sync_runners(&client, &state, device);

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
    fn icon_border_radius_ratio(&self) -> f32;
    fn displayname(&self) -> String;
    #[cfg(feature = "iced")]
    fn avatar_handle(&self) -> Avatar;
    #[cfg(feature = "iced")]
    fn small_avatar_handle(&self) -> Avatar;
    fn initial(&self) -> String;
    fn icon(&self) -> PolyIcon;
    fn color(&self) -> DePlaceColor;
}

pub trait OrderKey<T> {
    fn order_key(&self) -> (Option<String>, T);
}

impl OrderKey<Arc<OwnedUserId>> for Member {
    fn order_key(&self) -> (Option<String>, Arc<OwnedUserId>) {
        (self.raw_display_name_lowercase(), self.user_id.clone())
    }
}

impl ProfileLike for Member {
    fn icon_border_radius_ratio(&self) -> f32 {
        0.5
    }

    fn color(&self) -> DePlaceColor {
        self.name_color()
    }

    #[cfg(feature = "iced")]
    fn avatar_handle(&self) -> Avatar {
        self.avatar_handle()
    }

    #[cfg(feature = "iced")]
    fn small_avatar_handle(&self) -> Avatar {
        self.small_avatar_handle()
    }

    fn initial(&self) -> String {
        self.get_initial().to_string()
    }

    fn icon(&self) -> PolyIcon {
        self.polyicon()
    }

    fn displayname(&self) -> String {
        self.get_displayname()
    }
}

impl OrderKey<Arc<OwnedRoomId>> for DePlaceRoom {
    fn order_key(&self) -> (Option<String>, Arc<OwnedRoomId>) {
        (self.raw_displaystring_lowercase(), self.room_id.clone())
    }
}

impl ProfileLike for DePlaceRoom {
    fn icon_border_radius_ratio(&self) -> f32 {
        0.25
    }

    fn color(&self) -> DePlaceColor {
        if let Some(member) = self.other_dm_member() {
            member.color()
        } else {
            self.info.read().color
        }
    }

    fn initial(&self) -> String {
        self.info.read().initial.to_string()
    }

    fn icon(&self) -> PolyIcon {
        self.polyicon
    }

    fn displayname(&self) -> String {
        self.info.read().display_string.clone()
    }

    #[cfg(feature = "iced")]
    fn avatar_handle(&self) -> Avatar {
        self.info.read().avatar_handle.clone()
    }

    #[cfg(feature = "iced")]
    fn small_avatar_handle(&self) -> Avatar {
        self.info.read().small_avatar_handle.clone()
    }
}

impl ProfileLike for ActiveServer {
    fn icon_border_radius_ratio(&self) -> f32 {
        0.25
    }

    fn displayname(&self) -> String {
        match self {
            ActiveServer::Dms => "Direct Messages".into(),
            ActiveServer::Server(room) => room.displayname(),
        }
    }

    fn initial(&self) -> String {
        match self {
            ActiveServer::Dms => "D".into(),
            ActiveServer::Server(room) => room.initial(),
        }
    }

    #[cfg(feature = "iced")]
    fn avatar_handle(&self) -> Avatar {
        match self {
            ActiveServer::Dms => Avatar::NoAvatar,
            ActiveServer::Server(room) => room.avatar_handle(),
        }
    }

    #[cfg(feature = "iced")]
    fn small_avatar_handle(&self) -> Avatar {
        match self {
            ActiveServer::Dms => Avatar::NoAvatar,
            ActiveServer::Server(room) => room.small_avatar_handle(),
        }
    }

    fn icon(&self) -> PolyIcon {
        match self {
            ActiveServer::Dms => PolyIcon::from("dms"),
            ActiveServer::Server(room) => room.icon(),
        }
    }

    fn color(&self) -> DePlaceColor {
        match self {
            ActiveServer::Dms => DePlaceColor::from_hsla(0.0, 0.0, 0.0, 1.0),
            ActiveServer::Server(room) => room.color(),
        }
    }
}

impl ProfileLike for CallMember {
    fn icon_border_radius_ratio(&self) -> f32 {
        self.member.icon_border_radius_ratio()
    }

    fn displayname(&self) -> String {
        self.member.displayname()
    }

    #[cfg(feature = "iced")]
    fn avatar_handle(&self) -> Avatar {
        self.member.avatar_handle()
    }

    #[cfg(feature = "iced")]
    fn small_avatar_handle(&self) -> Avatar {
        self.member.small_avatar_handle()
    }

    fn color(&self) -> DePlaceColor {
        self.member.color()
    }

    fn initial(&self) -> String {
        self.member.initial().to_string()
    }

    fn icon(&self) -> PolyIcon {
        self.member.icon()
    }
}

pub fn window_title(room: Option<DePlaceRoom>, server: ActiveServer) -> String {
    let room_name = room.as_ref().map(|r| r.displayname());
    let server_name = server.as_server().map(|s| s.displayname());

    match (room_name, server_name) {
        (Some(room_name), Some(server_name)) => format!("#{} | {}", room_name, server_name),
        (Some(room_name), None) => format!("@{} - {APP_HUMAN_NAME}", room_name),
        (None, Some(server_name)) => format!("{} - {APP_HUMAN_NAME}", server_name),
        (None, None) => APP_HUMAN_NAME.to_string(),
    }
}

pub fn big_thumbnail_settings() -> MediaThumbnailSettings {
    MediaThumbnailSettings::new(uint!(100), uint!(100))
}

pub fn small_thumbnail_settings() -> MediaThumbnailSettings {
    MediaThumbnailSettings::new(uint!(25), uint!(25))
}

pub fn big_thumbnail_format() -> MediaFormat {
    MediaFormat::Thumbnail(big_thumbnail_settings())
}

pub fn small_thumbnail_format() -> MediaFormat {
    MediaFormat::Thumbnail(small_thumbnail_settings())
}
