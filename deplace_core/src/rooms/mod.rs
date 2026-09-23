use std::{collections::HashMap, iter::Sum, sync::Arc};

use futures::StreamExt;
use matrix_sdk::{
    Room,
    room::{ParentSpace, RoomMember},
    sync::UnreadNotificationsCount,
};
use ruma::{
    OwnedMxcUri, OwnedRoomId, OwnedUserId, RoomId, UserId,
    events::space::child::SpaceChildEventContent,
};

use crate::{ProfileLike, state::MembershipMap};

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DePlaceRoomType {
    Direct,
    Space,
    Call,
    Standard,
}

impl DePlaceRoomType {
    fn calculate(room: &Room) -> Self {
        if room.is_dm() {
            Self::Direct
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

    pub fn get_other_member(&self, map: &MembershipMap) -> Option<RoomMember> {
        map.get(&self.inner.id)?
            .values()
            .find(|m| m.user_id() != self.inner.own_id)
            .cloned()
    }

    pub fn is_dm(&self) -> bool {
        self.inner.room_type == DePlaceRoomType::Direct
    }

    pub fn is_call(&self) -> bool {
        self.inner.room_type == DePlaceRoomType::Call
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
            DePlaceRoomType::Direct => phosphor_svgs::icon::user::FILL,
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
}

impl DePlaceRoom {
    pub async fn from_room(room: Room) -> Self {
        let id = room.room_id().to_owned();

        let parent_spaces = match room.parent_spaces().await {
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

        Self {
            inner: Arc::new(DePlaceRoomInner {
                display_name: room.cached_display_name().map(|d| d.to_string()),
                room_type: DePlaceRoomType::calculate(&room),
                id,
                own_id: room.own_user_id().to_owned(),
                avatar_url: room.avatar_url(),
                notification_counts: room.unread_notification_counts().into(),
                parents: parent_spaces,
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

    const ICON_BORDER_RADIUS_RATIO: f32 = 0.25;

    fn profile_id(&self) -> Self::Id<'_> {
        self.room_id().to_owned()
    }

    fn profile_name(&self) -> Option<String> {
        self.display_name()
    }

    fn profile_avatar(&self) -> Option<OwnedMxcUri> {
        self.avatar_url()
    }
}
