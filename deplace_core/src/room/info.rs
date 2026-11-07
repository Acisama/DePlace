use std::sync::Arc;

use matrix_sdk::Room;
use ruma::{OwnedMxcUri, OwnedRoomId};

#[cfg(feature = "iced")]
use crate::Avatar;

use crate::colors::DePlaceColor;

#[derive(Clone, Debug)]
pub struct RoomInfo {
    room_id: Arc<OwnedRoomId>,

    pub(crate) display_name: Option<String>,
    pub(crate) display_string: String,
    pub(crate) initial: char,

    pub(crate) avatar_url: Option<OwnedMxcUri>,
    #[cfg(feature = "iced")]
    pub(crate) avatar_handle: Avatar,
    #[cfg(feature = "iced")]
    pub(crate) small_avatar_handle: Avatar,

    pub(crate) color: DePlaceColor,
}

impl PartialEq for RoomInfo {
    fn eq(&self, other: &Self) -> bool {
        self.room_id == other.room_id
            && self.display_name == other.display_name
            && self.avatar_url == other.avatar_url
    }
}

impl RoomInfo {
    pub fn new(room: &Room) -> Self {
        let room_id = room.room_id();

        let mut info = Self {
            room_id: Arc::new(room_id.to_owned()),

            display_name: room.cached_display_name().map(|d| d.to_string()),
            display_string: String::new(),
            initial: '?',

            avatar_url: room.avatar_url(),
            #[cfg(feature = "iced")]
            avatar_handle: Avatar::NoAvatar,
            #[cfg(feature = "iced")]
            small_avatar_handle: Avatar::NoAvatar,

            color: DePlaceColor::from(room_id.as_str()),
        };

        info.calculate_strings();
        info
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

    pub fn initial(&self) -> char {
        self.initial
    }

    pub fn color(&self) -> DePlaceColor {
        self.color
    }

    #[cfg(feature = "iced")]
    pub fn avatar_handle(&self) -> Avatar {
        self.avatar_handle.clone()
    }
}
