use std::{hash::Hash, sync::Arc};

use dashmap::{DashMap, mapref::entry::Entry};
use ruma::{OwnedRoomId, OwnedUserId, UserId, events::room::member::MembershipState};

use crate::Shared;

use super::{
    Member,
    profile::ProfileData,
    user_data::{Presence, UserData},
};

#[derive(Debug)]
pub struct RoomMembers {
    room_id: Arc<OwnedRoomId>,

    /// Every member ever seen in this room, including former ones
    by_id: DashMap<OwnedUserId, Member>,
    /// Active members, sorted for the member list
    active: Shared<Vec<Member>>,
    /// Active members which are also online
    online: Shared<Vec<Member>>,
    /// Active members which are offline
    offline: Shared<Vec<Member>>,
}

impl Hash for RoomMembers {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.active().iter().for_each(|m| m.hash_all(h));
    }
}

impl RoomMembers {
    pub fn new(room_id: OwnedRoomId) -> Self {
        Self {
            room_id: Arc::new(room_id),
            by_id: DashMap::new(),
            active: Shared::new(Vec::new()),
            online: Shared::new(Vec::new()),
            offline: Shared::new(Vec::new()),
        }
    }

    pub fn get(&self, user_id: &UserId) -> Option<Member> {
        self.by_id.get(user_id).map(|m| m.clone())
    }

    pub fn active(&self) -> Arc<Vec<Member>> {
        self.active.get()
    }

    pub fn online(&self) -> Arc<Vec<Member>> {
        self.online.get()
    }

    pub fn online_len(&self) -> usize {
        self.online.data.load().len()
    }

    pub fn offline_len(&self) -> usize {
        self.offline.data.load().len()
    }

    pub fn active_len(&self) -> usize {
        self.active.data.load().len()
    }

    pub fn offline(&self) -> Arc<Vec<Member>> {
        self.offline.get()
    }

    pub fn active_version(&self) -> u64 {
        self.active.version()
    }

    /// Get or create. bool = newly created (the caller starts the load)
    pub fn get_or_create(
        &self,
        user_id: Arc<OwnedUserId>,
        user_data: Arc<Shared<UserData>>,
    ) -> (Member, bool) {
        if let Some(m) = self.by_id.get(user_id.as_ref()) {
            return (m.clone(), false);
        }

        match self.by_id.entry((*user_id).clone()) {
            Entry::Occupied(e) => (e.get().clone(), false),
            Entry::Vacant(e) => {
                let m = Member {
                    room_data: Arc::new(Shared::new(ProfileData::new(user_id.clone()))),
                    room_id: self.room_id.clone(),
                    user_id,
                    user_data,
                };
                e.insert(m.clone());
                (m, true)
            }
        }
    }

    /// Sync path: apply a profile update, rebuilding the sorted lists if it affects
    /// membership or the display name used to sort them
    pub(crate) fn update_member(&self, member: &Member, f: impl Fn(&mut ProfileData)) {
        let old = member.room_data.get();
        member.update_room_profile(f);
        let new = member.room_data.get();

        if old.membership != new.membership || old.displayname != new.displayname {
            self.rebuild_list();
        }
    }

    fn rebuild_list(&self) {
        let mut v: Vec<Member> = self
            .by_id
            .iter()
            .filter(|e| {
                matches!(
                    e.value().room_data.get().membership,
                    MembershipState::Join | MembershipState::Invite
                )
            })
            .map(|e| e.value().clone())
            .collect();
        v.sort_by_cached_key(|m| (m.raw_display_name_lowercase(), m.user_id.clone()));
        self.active.set(v.clone());
        let (online, offline) = v
            .into_iter()
            .partition(|m| m.presence() != Presence::Offline);
        self.online.set_if_modified(online);
        self.offline.set_if_modified(offline);
    }

    pub(crate) fn get_first_other(&self, own_user_id: &UserId) -> Option<Member> {
        self.active
            .get()
            .iter()
            .find(|m| m.user_id.as_ref() != own_user_id)
            .cloned()
    }

    pub(crate) fn extend(&self, members: Vec<Member>) {
        for member in members {
            self.by_id.insert(member.user_id().to_owned(), member);
        }
        self.rebuild_list();
    }
}
