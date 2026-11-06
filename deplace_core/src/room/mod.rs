use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use live::RoomLive;
use matrix_sdk::Room;
use ruma::{OwnedRoomId, RoomId, UserId};
use tokio::sync::OnceCell;

use crate::{
    Shared,
    call::RoomCall,
    member::{Member, ProfileStore, room::RoomMembers},
    state::UserDevice,
};

pub mod call;
pub mod hierarchy;
pub mod info;
pub mod live;
pub mod notifications;
pub mod store;
pub mod timelines;

use info::RoomInfo;
use timelines::RoomTimelines;

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
    pub(crate) room_id: Arc<OwnedRoomId>,
    room_type: DePlaceRoomType,
    pub(crate) info: Arc<Shared<RoomInfo>>,
    pub(crate) live: Arc<Shared<RoomLive>>,
    pub(crate) members: Arc<RoomMembers>,
    timelines: Arc<OnceCell<RoomTimelines>>,
    pub(crate) call: Arc<RoomCall>,
}

impl DePlaceRoom {
    pub fn new(room: &Room, profile_store: &ProfileStore, own_device: Arc<UserDevice>) -> Self {
        let room_id = room.room_id();
        let members = profile_store.get_members(room_id);

        Self {
            room_id: Arc::new(room_id.to_owned()),
            room: room.clone(),
            room_type: DePlaceRoomType::new(room),
            info: Arc::new(Shared::new(RoomInfo::new(room))),
            live: Arc::new(Shared::new(RoomLive::new(room))),
            members,
            timelines: Arc::new(OnceCell::new()),
            call: Arc::new(RoomCall::new(own_device)),
        }
    }

    /// Returns this room's live and pinned-events timelines, building them
    /// on first call and reusing the cached result afterwards. Does not
    /// paginate -- callers that need more than what's already locally
    /// cached are responsible for that themselves.
    pub async fn timelines(&self) -> anyhow::Result<RoomTimelines> {
        self.timelines
            .get_or_try_init(|| RoomTimelines::build(&self.room))
            .await
            .cloned()
    }

    pub fn hash_selective<H: Hasher>(
        &self,
        state: &mut H,
        info: bool,
        live: bool,
        members: bool,
        call: bool,
    ) {
        self.room_id.hash(state);

        if members {
            self.members.hash(state);
        }
        if info {
            self.info.version().hash(state);
        }
        if live {
            self.live.version().hash(state);
        }
        if call {
            self.call.members.read().hash(state);
        }
    }

    pub fn hash_all<H: Hasher>(&self, state: &mut H) {
        self.hash_selective(state, true, true, true, true);
    }

    pub fn room_id(&self) -> &RoomId {
        &self.room_id
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

    pub fn members_ref(&self) -> &RoomMembers {
        &self.members
    }

    pub fn members(&self) -> Arc<RoomMembers> {
        self.members.clone()
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

    pub(crate) fn raw_displaystring_lowercase(&self) -> Option<String> {
        self.info
            .read()
            .display_name
            .clone()
            .map(|s| s.to_lowercase())
    }

    pub fn call(&self) -> &RoomCall {
        &self.call
    }

    pub fn has_call(&self) -> bool {
        self.call().call_participants_count() > 0
    }

    pub fn any_is_screensharing(&self) -> bool {
        self.has_call()
            && self
                .call
                .members
                .read()
                .members
                .iter()
                .any(|m| m.is_screensharing())
    }
}
