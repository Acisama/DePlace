use std::sync::Arc;

use matrix_sdk::Room;
use ruma::{OwnedMxcUri, OwnedRoomId};

use crate::{
    cache::{ProfileAvatars, ThumbnailCache},
    colors::DePlaceColor,
};

#[derive(Clone)]
pub struct RoomInfo {
    room_id: Arc<OwnedRoomId>,

    pub(crate) display_name: Option<String>,
    pub(crate) display_string: String,
    pub(crate) initial: char,

    pub(crate) avatar_url: Option<OwnedMxcUri>,
    pub(crate) color: DePlaceColor,

    avatars: Option<ProfileAvatars>,
}

impl PartialEq for RoomInfo {
    fn eq(&self, other: &Self) -> bool {
        self.room_id == other.room_id
            && self.display_name == other.display_name
            && self.avatar_url == other.avatar_url
    }
}

impl RoomInfo {
    pub fn new(room: &Room, cache: &ThumbnailCache, eagerly_load: bool) -> Self {
        let room_id = room.room_id();
        let avatar_url = room.avatar_url();
        let avatars = avatar_url.map(|url| ProfileAvatars::get(cache, url));

        if eagerly_load && let Some(avatars) = &avatars {
            avatars.start_load_all();
        }

        let mut info = Self {
            room_id: Arc::new(room_id.to_owned()),

            display_name: room.cached_display_name().map(|d| d.to_string()),
            display_string: String::new(),
            initial: '?',

            avatar_url: room.avatar_url(),
            avatars,

            color: DePlaceColor::from(room_id.as_str()),
        };

        info.calculate_strings();
        info
    }

    pub fn update_avatar(&mut self, avatar_url: Option<OwnedMxcUri>, cache: &ThumbnailCache) {
        self.avatar_url = avatar_url.clone();
        self.avatars = avatar_url.map(|url| ProfileAvatars::get(cache, url));
    }

    pub fn avatar_url(&self) -> Option<OwnedMxcUri> {
        self.avatar_url.clone()
    }

    pub(crate) fn calculate_strings(&mut self) {
        if let Some(name) = &self.display_name {
            self.display_string = name.clone();
        } else {
            self.display_string = self.room_id.to_string();
        }
        self.initial = self
            .display_string
            .chars()
            .next()
            .and_then(|c| c.to_uppercase().next())
            .unwrap_or('?');
    }

    pub fn display_string(&self) -> String {
        self.display_string.clone()
    }

    pub fn avatars(&self) -> Option<ProfileAvatars> {
        self.avatars.clone()
    }

    pub fn initial(&self) -> char {
        self.initial
    }

    pub fn color(&self) -> DePlaceColor {
        self.color
    }
}
