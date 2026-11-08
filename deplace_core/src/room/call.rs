use std::{hash::Hash, sync::Arc, time::SystemTime};

use indexmap::IndexSet;
use matrix_sdk::{Room, deserialized_responses::SyncOrStrippedState};
use rodio::buffer::SamplesBuffer;
use ruma::{
    OwnedDeviceId,
    events::{
        call::member::{CallMemberEventContent, MembershipData},
        rtc::notification::CallIntent,
    },
};

use crate::{
    Member, OrderKey,
    call::{CallParticipantHandle, RoomCall, VolumeCell},
    member::{ProfileStore, synth::SignatureEvent},
    state::UserDevice,
};

#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub struct CallMembers {
    pub(crate) members: Vec<CallMember>,
    pub(crate) own_user_is_in_call: bool,
    pub(crate) own_user_device_is_in_call: bool,
}

impl CallMembers {
    pub(crate) async fn new(
        room: &Room,
        profile_store: &ProfileStore,
        call: &RoomCall,
        own_device: &UserDevice,
    ) -> Self {
        let mut call_participants = IndexSet::new();

        let room_id = room.room_id();

        if room.has_active_room_call() {
            let events = match room
                .get_state_events_static::<CallMemberEventContent>()
                .await
            {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!("Failed to get call member events: {e}");
                    Vec::new()
                }
            };

            for raw in events {
                let Ok(SyncOrStrippedState::Sync(ev)) = raw.deserialize() else {
                    continue;
                };
                let matrix_sdk::ruma::events::SyncStateEvent::Original(ev) = ev else {
                    continue;
                };
                let member = profile_store.get_member(room_id, ev.state_key.user_id());

                for membership in ev.content.active_memberships(Some(ev.origin_server_ts)) {
                    call_participants.insert(CallMember::new(&membership, member.clone(), call));
                }
            }
        }

        call_participants.sort_by_cached_key(|p| p.member.order_key());

        let mut members = CallMembers {
            members: call_participants.into_iter().collect(),
            own_user_is_in_call: false,
            own_user_device_is_in_call: false,
        };
        members.calculate_own_call(own_device);
        members
    }

    pub(crate) fn calculate_own_call(&mut self, own_device: &UserDevice) {
        self.own_user_is_in_call = false;
        self.own_user_device_is_in_call = false;

        for part in &self.members {
            if self.own_user_device_is_in_call && self.own_user_is_in_call {
                return;
            }

            if part.member.user_id() != own_device.user_id {
                continue;
            }

            self.own_user_is_in_call = true;
            if part.device_id.as_ref() == &own_device.device_id {
                self.own_user_device_is_in_call = true;
            }
        }
    }

    pub fn own_user_is_in_call(&self) -> bool {
        self.own_user_is_in_call
    }

    pub fn own_user_device_is_in_call(&self) -> bool {
        self.own_user_device_is_in_call
    }
}

#[derive(Debug, Clone)]
pub struct CallMember {
    pub member: Member,
    pub device_id: Arc<OwnedDeviceId>,
    pub call_intent: Arc<CallIntent>,
    pub created: Option<SystemTime>,

    handle: CallParticipantHandle,
}
impl Hash for CallMember {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.member.hash_selective(state, false, true, true, true);
        self.device_id.hash(state);
        self.handle.participant.hash(state);
        self.handle
            .playback_muted
            .load(std::sync::atomic::Ordering::Relaxed)
            .hash(state);
    }
}

impl PartialEq for CallMember {
    fn eq(&self, other: &Self) -> bool {
        self.member == other.member && self.device_id == other.device_id
    }
}

impl Eq for CallMember {}

impl CallMember {
    /// A stand-in for our own entry while a join is in progress but our
    /// membership event hasn't been synced into the members yet, so
    /// the join progress can be rendered from the moment the join was
    /// started.
    pub(crate) fn own_pending(
        member: Member,
        own_device: &UserDevice,
        handle: CallParticipantHandle,
    ) -> Self {
        Self {
            member,
            device_id: Arc::new(own_device.device_id.clone()),
            call_intent: Arc::new(CallIntent::Audio),
            created: None,

            handle,
        }
    }

    pub(crate) fn new(membership: &MembershipData, member: Member, call: &RoomCall) -> Self {
        let user_id = member.user_id().to_owned();
        let device_id = membership.device_id().to_owned();

        let handle = call.call_handle(user_id, device_id.clone());

        Self {
            member,
            device_id: Arc::new(device_id),
            call_intent: Arc::new(
                membership
                    .call_intent()
                    .cloned()
                    .unwrap_or(CallIntent::Audio),
            ),
            created: membership.created_ts().and_then(|s| s.to_system_time()),

            handle,
        }
    }

    pub fn volume(&self) -> Arc<VolumeCell> {
        self.handle.volume.clone()
    }

    /// The call handle of this participant, for the UI to read and
    /// adjust how loud they play for us.
    pub fn handle(&self) -> &CallParticipantHandle {
        &self.handle
    }

    pub fn calc_synth_sound(&self, event: SignatureEvent) -> SamplesBuffer<f32> {
        self.member.calc_synth_sound(event)
    }

    pub fn is_muted(&self) -> bool {
        self.handle.participant.read().muted
    }

    pub fn playback_muted(&self) -> bool {
        self.handle.playback_muted()
    }

    pub fn is_speaking(&self) -> bool {
        !self.is_muted() && self.handle.participant.read().speaking
    }

    pub fn is_screensharing(&self) -> bool {
        self.handle.participant.read().screensharing
    }

    pub fn is_own(&self, own_device: &UserDevice) -> bool {
        self.member.user_id() == own_device.user_id
            && self.device_id.as_ref() == &own_device.device_id
    }
}
