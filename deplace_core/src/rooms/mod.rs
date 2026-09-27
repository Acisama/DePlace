use std::{
    collections::{BTreeSet, HashMap},
    hash::Hash,
    iter::Sum,
    sync::Arc,
};

use futures::StreamExt;
use matrix_sdk::{
    Room, RoomMemberships,
    deserialized_responses::SyncOrStrippedState,
    room::{ParentSpace, RoomMember},
    sync::UnreadNotificationsCount,
};
use ruma::{
    MilliSecondsSinceUnixEpoch, OwnedDeviceId, OwnedMxcUri, OwnedRoomId, OwnedUserId, RoomId,
    UserId,
    events::{
        call::member::CallMemberEventContent, rtc::notification::CallIntent,
        space::child::SpaceChildEventContent,
    },
};

use crate::{ProfileLike, colors::DePlaceColor, state::UserDevice};

#[derive(Debug, Clone)]
pub struct DePlaceRoom {
    pub room: Room,
    inner: Arc<DePlaceRoomInner>,
}

pub mod hashing;
pub mod hierarchy;
pub mod watchers;
pub use hashing::RoomWatcherHashingConfig;
pub use hierarchy::SpaceHierarchy;
pub use watchers::{DmRoomMap, GenericRoomMap, RoomWatchers};

#[derive(Debug, Clone)]
pub enum DePlaceRoomType {
    Direct { other_member: Option<RoomMember> },
    Space,
    Call,
    Standard,
}

impl DePlaceRoomType {
    async fn calculate(room: &Room) -> Self {
        if room.is_dm() {
            let members = room
                .members(RoomMemberships::ACTIVE)
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Failed to get room members for room {}: {e}",
                        room.room_id()
                    )
                })
                .unwrap_or_default();

            let own_id = room.own_user_id();

            Self::Direct {
                other_member: members.iter().find(|m| m.user_id() != own_id).cloned(),
            }
        } else if room.is_call() {
            Self::Call
        } else if room.is_space() {
            Self::Space
        } else {
            Self::Standard
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Hash)]
pub struct NotificationCounts {
    pub highlight_count: u64,
    pub notification_count: u64,
}

impl NotificationCounts {
    pub fn total(&self) -> u64 {
        self.highlight_count.max(self.notification_count)
    }

    pub fn highlights(&self) -> Option<u64> {
        if self.highlight_count > 0 {
            Some(self.highlight_count)
        } else {
            None
        }
    }

    pub fn has_notifications(&self) -> bool {
        self.highlight_count > 0 || self.notification_count > 0
    }
}

impl From<UnreadNotificationsCount> for NotificationCounts {
    fn from(value: UnreadNotificationsCount) -> Self {
        Self {
            highlight_count: value.highlight_count,
            notification_count: value.notification_count,
        }
    }
}

impl Sum for NotificationCounts {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |acc, counts| Self {
            highlight_count: acc.highlight_count + counts.highlight_count,
            notification_count: acc.notification_count + counts.notification_count,
        })
    }
}

#[derive(Debug)]
struct DePlaceRoomInner {
    id: OwnedRoomId,
    room_type: DePlaceRoomType,
    own_id: OwnedUserId,
    display_name: Option<String>,
    avatar_url: Option<OwnedMxcUri>,
    notification_counts: NotificationCounts,
    parents: HashMap<OwnedRoomId, Option<String>>,
    call_participants: BTreeSet<Arc<CallMember>>,
    own_user_is_in_call: bool,
    own_user_device_is_in_call: bool,
    color: DePlaceColor,
}

impl DePlaceRoom {
    pub fn room_id(&self) -> &RoomId {
        &self.inner.id
    }

    pub fn display_name(&self) -> Option<String> {
        self.inner.display_name.clone()
    }

    pub fn avatar_url(&self) -> Option<OwnedMxcUri> {
        self.inner.avatar_url.clone()
    }

    pub fn is_dm(&self) -> bool {
        matches!(self.inner.room_type, DePlaceRoomType::Direct { .. })
    }

    pub fn is_call(&self) -> bool {
        matches!(self.inner.room_type, DePlaceRoomType::Call)
    }

    pub fn is_space(&self) -> bool {
        matches!(self.inner.room_type, DePlaceRoomType::Space)
    }

    pub fn parents(&self) -> HashMap<OwnedRoomId, Option<String>> {
        self.inner.parents.clone()
    }

    pub fn icon(&self) -> &'static str {
        match self.inner.room_type {
            DePlaceRoomType::Call => phosphor_svgs::icon::speaker_high::FILL,
            DePlaceRoomType::Direct { .. } => phosphor_svgs::icon::user::FILL,
            DePlaceRoomType::Standard => phosphor_svgs::icon::hash::BOLD,
            DePlaceRoomType::Space => phosphor_svgs::icon::planet::BOLD,
        }
    }

    pub fn sdk_room(&self) -> &Room {
        &self.room
    }

    pub fn notification_counts(&self) -> NotificationCounts {
        self.inner.notification_counts
    }

    pub fn own_user_id(&self) -> &UserId {
        &self.inner.own_id
    }

    pub fn is_user_in_call(&self, user_id: &UserId) -> bool {
        self.inner
            .call_participants
            .iter()
            .any(|member| member.member.user_id() == user_id)
    }

    pub fn is_user_device_in_call(&self, device: &UserDevice) -> bool {
        self.inner.call_participants.iter().any(|member| {
            member.member.user_id() == device.user_id && member.device_id == device.device_id
        })
    }

    pub fn own_user_is_in_call(&self) -> bool {
        self.inner.own_user_is_in_call
    }

    pub fn own_user_device_is_in_call(&self) -> bool {
        self.inner.own_user_device_is_in_call
    }

    pub fn dm_other_member(&self) -> Option<RoomMember> {
        match &self.inner.room_type {
            DePlaceRoomType::Direct { other_member } => other_member.clone(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallMember {
    pub member: RoomMember,
    pub device_id: OwnedDeviceId,
    pub call_intent: CallIntent,
    created_ts: Option<MilliSecondsSinceUnixEpoch>,
}

impl Hash for CallMember {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.member.user_id().hash(state);
        self.device_id.hash(state);
    }
}

impl PartialEq for CallMember {
    fn eq(&self, other: &Self) -> bool {
        self.member.user_id() == other.member.user_id() && self.device_id == other.device_id
    }
}
impl Eq for CallMember {}

impl PartialOrd for CallMember {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CallMember {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.created_ts
            .cmp(&other.created_ts)
            .then_with(|| self.member.user_id().cmp(other.member.user_id()))
            .then_with(|| self.device_id.cmp(&other.device_id))
    }
}

impl DePlaceRoom {
    pub async fn from_room(room: Room, own_device: &UserDevice) -> Self {
        let id = room.room_id().to_owned();

        #[allow(clippy::mutable_key_type)]
        let mut call_participants: BTreeSet<Arc<CallMember>> = BTreeSet::new();

        if let Ok(events) = room
            .get_state_events_static::<CallMemberEventContent>()
            .await
        {
            for raw in events {
                let Ok(SyncOrStrippedState::Sync(ev)) = raw.deserialize() else {
                    continue;
                };
                let matrix_sdk::ruma::events::SyncStateEvent::Original(ev) = ev else {
                    continue;
                };
                let user_id = ev.state_key.user_id();

                let member = match room.get_member(user_id).await {
                    Ok(Some(m)) => m,
                    Ok(None) => {
                        tracing::warn!(
                            "Call participant with user id {} not found in room {}",
                            user_id,
                            id
                        );
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to get member for user id {}: {}", user_id, e);
                        continue;
                    }
                };

                for membership in ev.content.active_memberships(None) {
                    call_participants.insert(Arc::new(CallMember {
                        member: member.clone(),
                        device_id: membership.device_id().to_owned(),
                        call_intent: membership
                            .call_intent()
                            .cloned()
                            .unwrap_or(CallIntent::Audio),
                        created_ts: membership.created_ts(),
                    }));
                }
            }
        }

        let parents = match room.parent_spaces().await {
            Ok(stream) => {
                let parents_res = stream.collect::<Vec<_>>().await;

                let mut parents = HashMap::new();

                for res in parents_res {
                    let parent_room = match res {
                        Ok(ParentSpace::Reciprocal(room)) => room,
                        Err(e) => {
                            tracing::error!("Failed to get parent space: {:?}", e);
                            continue;
                        }
                        _ => continue,
                    };

                    let order_string = match parent_room
                        .get_state_event_static_for_key::<SpaceChildEventContent, _>(&id)
                        .await
                    {
                        Ok(child) => child
                            .and_then(|raw| match raw.deserialize() {
                                Ok(child) => child.as_sync().cloned(),
                                Err(e) => {
                                    tracing::error!("Failed to deserialize child event: {:?}", e);
                                    None
                                }
                            })
                            .and_then(|v| v.as_original().cloned())
                            .and_then(|v| v.content.order.clone())
                            .map(|o| o.to_string()),
                        Err(e) => {
                            tracing::error!("Failed to get child event: {:?}", e);
                            None
                        }
                    };

                    parents.insert(parent_room.room_id().to_owned(), order_string);
                }

                parents
            }
            Err(e) => {
                tracing::error!("Failed to get parent spaces: {:?}", e);
                HashMap::new()
            }
        };

        let room_type = DePlaceRoomType::calculate(&room).await;

        let (display_name, avatar_url, color) =
            if let DePlaceRoomType::Direct { other_member } = &room_type {
                (
                    Some(other_member.get_name()),
                    other_member.get_avatar(),
                    if let Some(member) = &other_member {
                        member.color()
                    } else {
                        room.color()
                    },
                )
            } else {
                (
                    room.cached_display_name().map(|d| d.to_string()),
                    room.avatar_url(),
                    room.color(),
                )
            };

        Self {
            inner: Arc::new(DePlaceRoomInner {
                display_name,
                room_type: DePlaceRoomType::calculate(&room).await,
                id,
                own_id: room.own_user_id().to_owned(),
                avatar_url,
                notification_counts: room.unread_notification_counts().into(),
                parents,
                own_user_is_in_call: call_participants
                    .iter()
                    .any(|p| p.member.user_id() == own_device.user_id),
                own_user_device_is_in_call: call_participants.iter().any(|p| {
                    p.member.user_id() == own_device.user_id && p.device_id == own_device.device_id
                }),
                call_participants,
                color,
            }),
            room,
        }
    }
}

impl ProfileLike for DePlaceRoom {
    type Id<'a>
        = OwnedRoomId
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32 {
        if self.is_dm() { 0.5 } else { 0.25 }
    }

    fn profile_id(&self) -> Self::Id<'_> {
        self.room_id().to_owned()
    }

    fn profile_name(&self) -> Option<String> {
        self.display_name()
    }

    fn get_avatar(&self) -> Option<OwnedMxcUri> {
        self.avatar_url()
    }

    fn color(&self) -> DePlaceColor {
        self.inner.color
    }
}
