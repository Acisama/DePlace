use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use live::RoomLive;
use matrix_sdk::Room;
use ruma::{OwnedRoomId, RoomId, UserId};

use crate::{
    Shared,
    member::{Member, ProfileStore, room::RoomMembers},
};

pub mod hierarchy;
pub mod info;
pub mod live;
pub mod notifications;
pub mod store;

use info::RoomInfo;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DePlaceRoomType {
    Direct,
    Space,
    Call,
    #[default]
    Standard,
}

impl DePlaceRoomType {
    pub fn new(room: &Room) -> Self {
        if room.is_dm() {
            Self::Direct
        } else if room.is_space() {
            Self::Space
        } else if room.is_call() {
            Self::Call
        } else {
            Self::Standard
        }
    }

    pub fn is_direct(&self) -> bool {
        matches!(self, DePlaceRoomType::Direct)
    }

    pub fn is_space(&self) -> bool {
        matches!(self, DePlaceRoomType::Space)
    }

    pub fn is_call(&self) -> bool {
        matches!(self, DePlaceRoomType::Call)
    }

    pub fn is_standard(&self) -> bool {
        matches!(self, DePlaceRoomType::Standard)
    }
}

#[derive(Debug, Clone)]
pub struct DePlaceRoom {
    pub room: Room,
    id: Arc<OwnedRoomId>,
    room_type: DePlaceRoomType,
    pub(crate) info: Arc<Shared<RoomInfo>>,
    pub(crate) live: Arc<Shared<RoomLive>>,
    pub(crate) members: Arc<RoomMembers>,
}

impl DePlaceRoom {
    pub fn new(room: &Room, profile_store: &ProfileStore) -> Self {
        let room_id = room.room_id();
        let members = profile_store.get_members(room_id);

        Self {
            id: Arc::new(room_id.to_owned()),
            room: room.clone(),
            room_type: DePlaceRoomType::new(room),
            info: Arc::new(Shared::new(RoomInfo::new(room))),
            live: Arc::new(Shared::new(RoomLive::new(room))),
            members,
        }
    }

    pub fn hash_selective<H: Hasher>(&self, state: &mut H, info: bool, live: bool, members: bool) {
        self.id.hash(state);

        if members {
            self.members.hash(state);
        }
        if info {
            self.info.version().hash(state);
        }
        if live {
            self.live.version().hash(state);
        }
    }

    pub fn hash_all<H: Hasher>(&self, state: &mut H) {
        self.hash_selective(state, true, true, true);
    }

    pub fn room_id(&self) -> &RoomId {
        &self.id
    }

    pub fn own_user_id(&self) -> &UserId {
        self.room.own_user_id()
    }

    pub fn info(&self) -> Arc<RoomInfo> {
        self.info.get()
    }

    pub fn live(&self) -> Arc<RoomLive> {
        self.live.get()
    }

    pub(crate) fn update_info(&self, f: impl Fn(&mut RoomInfo)) {
        self.info.change(f);
    }

    pub(crate) fn update_live_if_modified(&self, f: impl Fn(&mut RoomLive)) {
        self.live.change_if_modified(f);
    }

    pub fn members(&self) -> &RoomMembers {
        &self.members
    }

    /// The other party in a DM, read live off the current member list rather
    /// than cached at room-creation time - members load in asynchronously,
    /// so a one-shot snapshot would almost always see an empty list.
    pub fn other_dm_member(&self) -> Option<Member> {
        if !self.room_type.is_direct() {
            return None;
        }
        self.members.get_first_other(self.room.own_user_id())
    }

    pub fn room_type(&self) -> DePlaceRoomType {
        self.room_type
    }

    pub fn has_active_call(&self) -> bool {
        !self.live.data.load().call_participants.is_empty()
    }

    pub fn get_input_placeholder(&self) -> String {
        let name = self.info.data.load().display_string();
        if self.room_type.is_direct() {
            format!("@{name}")
        } else {
            format!("#{name}")
        }
    }

    pub fn is_dm(&self) -> bool {
        self.room_type.is_direct()
    }
}
