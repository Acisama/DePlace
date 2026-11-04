use std::{hash::Hash, sync::Arc, time::SystemTime};

use indexmap::IndexSet;
use matrix_sdk::Room;
use ruma::{
    OwnedDeviceId,
    events::{call::member::MembershipData, rtc::notification::CallIntent},
};

use crate::member::Member;

use super::notifications::NotificationCounts;

#[derive(Debug, Clone)]
pub struct CallMember {
    pub member: Member,
    pub device_id: Arc<OwnedDeviceId>,
    pub call_intent: Arc<CallIntent>,
    pub created: Option<SystemTime>,
}

impl Hash for CallMember {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.member.hash_selective(state, false, true, true, true);
        self.device_id.hash(state);
    }
}

impl PartialEq for CallMember {
    fn eq(&self, other: &Self) -> bool {
        self.member == other.member && self.device_id == other.device_id
    }
}

impl Eq for CallMember {}

impl CallMember {
    pub(crate) fn new(membership: &MembershipData, member: Member) -> Self {
        Self {
            member,
            device_id: Arc::new(membership.device_id().to_owned()),
            call_intent: Arc::new(
                membership
                    .call_intent()
                    .cloned()
                    .unwrap_or(CallIntent::Audio),
            ),
            created: membership.created_ts().and_then(|s| s.to_system_time()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoomLive {
    pub(crate) notification_counts: NotificationCounts,
    pub(crate) call_participants: IndexSet<CallMember>,
    pub(crate) own_user_is_in_call: bool,
    pub(crate) own_user_device_is_in_call: bool,
}

impl RoomLive {
    pub fn new(room: &Room) -> Self {
        Self {
            notification_counts: room.unread_notification_counts().into(),
            call_participants: IndexSet::new(),
            own_user_is_in_call: false,
            own_user_device_is_in_call: false,
        }
    }

    pub fn notification_counts(&self) -> NotificationCounts {
        self.notification_counts
    }

    pub fn call_participants(&self) -> IndexSet<CallMember> {
        self.call_participants.clone()
    }

    pub fn call_participants_count(&self) -> usize {
        self.call_participants.len()
    }

    pub fn has_call_participants(&self) -> bool {
        !self.call_participants.is_empty()
    }

    pub fn own_user_is_in_call(&self) -> bool {
        self.own_user_is_in_call
    }

    pub fn own_user_device_is_in_call(&self) -> bool {
        self.own_user_device_is_in_call
    }
}
