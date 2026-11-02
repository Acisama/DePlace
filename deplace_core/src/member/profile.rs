#[cfg(feature = "iced")]
use std::sync::Arc;

use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    room::RoomMember,
};
use ruma::{
    OwnedMxcUri, UserId,
    events::room::{
        MediaSource,
        member::MembershipState,
        power_levels::UserPowerLevel::{self},
    },
    uint,
};

#[cfg(feature = "iced")]
use iced::widget::image::Handle as ImageHandle;

use super::Shared;

#[derive(Clone, Debug)]
pub struct ProfileData {
    pub(super) avatar_url: Option<OwnedMxcUri>,

    pub(super) display_name: Option<String>,
    pub(super) display_string: String,
    pub(super) initial: char,

    pub(super) membership: MembershipState,
    pub(super) power_level: UserPowerLevel,
    #[cfg(feature = "iced")]
    pub(super) avatar_handle: Option<Result<ImageHandle, Arc<String>>>,
}

impl ProfileData {
    pub fn new(user_id: &UserId) -> Self {
        Self {
            avatar_url: None,

            display_name: None,
            display_string: user_id.to_string(),
            initial: '?',

            membership: MembershipState::Join,
            power_level: UserPowerLevel::Infinite,
            #[cfg(feature = "iced")]
            avatar_handle: None,
        }
    }
}

impl From<&RoomMember> for ProfileData {
    fn from(m: &RoomMember) -> Self {
        let display_name = m.display_name().map(|n| n.to_owned());
        let display_string = display_name
            .clone()
            .unwrap_or_else(|| m.user_id().to_string());
        let initial = display_string.chars().next().unwrap_or('?');

        Self {
            avatar_url: m.avatar_url().map(|u| u.to_owned()),

            display_string,
            display_name,
            initial,

            membership: m.membership().clone(),
            power_level: m.power_level(),
            #[cfg(feature = "iced")]
            avatar_handle: None,
        }
    }
}

impl ProfileData {
    pub fn avatar_url(&self) -> Option<&OwnedMxcUri> {
        self.avatar_url.as_ref()
    }

    pub fn set_avatar_handle(&mut self, handle: Option<Result<ImageHandle, Arc<String>>>) {
        self.avatar_handle = handle;
    }
}

pub async fn load_avatar(client: &Client, cell: &Shared<ProfileData>, url: OwnedMxcUri) {
    let request = MediaRequestParameters {
        source: MediaSource::Plain(url.clone()),
        format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(uint!(100), uint!(100))),
    };
    let handle = client
        .media()
        .get_media_content(&request, true)
        .await
        .map(ImageHandle::from_bytes)
        .map_err(|e| Arc::new(e.to_string()));

    cell.modify(|d| {
        if d.avatar_url() == Some(&url) {
            d.avatar_handle = Some(handle.clone());
        }
    });
}
