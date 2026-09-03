use crate::common::*;
use std::time::SystemTime;

use macros::iced_cache;
use matrix_sdk::{
    media::UniqueKey,
    ruma::events::{room::MediaSource, rtc::notification::CallIntent},
};
use matrix_sdk_ui::timeline::{
    EventSendState, MemberProfileChange, MsgLikeKind, OtherState, Profile, ReactionsByKeyBySender,
    RoomMembershipChange, TimelineDetails, TimelineEventShieldState,
};

mod convert;
mod render;

pub use convert::ToTimelineItem;

#[derive(Debug, Clone)]
pub enum TimelineItemMessage {
    NeedsAvatar(OwnedMxcUri),
    NeedsThumbnail {
        source: MediaSource,
        width: u64,
        height: u64,
    },
    None,
}

impl NeedsAvatarExt for TimelineItemMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        TimelineItemMessage::NeedsAvatar(uri)
    }
}

pub enum TimelineItemAction {
    NeedsAvatar(OwnedMxcUri),
    NeedsThumbnail {
        source: MediaSource,
        width: u64,
        height: u64,
    },
    None,
}

#[iced_cache(Debug, Clone)]
pub struct TimelineItem {
    #[hash]
    pub id: String,
    #[hash]
    kind: TimelineItemKind,

    avatar_cache: AvatarCache,
    thumbnail_cache: ThumbnailCache,
}

impl IcedWidget<TimelineItemMessage, TimelineItemAction> for TimelineItem {
    fn update(&mut self, message: TimelineItemMessage) -> TimelineItemAction {
        match message {
            TimelineItemMessage::NeedsAvatar(uri) => {
                self.retain_avatar_hashes_no_task(uri.clone());
                TimelineItemAction::NeedsAvatar(uri)
            }
            TimelineItemMessage::NeedsThumbnail {
                source,
                width,
                height,
            } => {
                self.retain_thumbnail_hashes_no_task((source.unique_key(), width, height).clone());
                TimelineItemAction::NeedsThumbnail {
                    source,
                    width,
                    height,
                }
            }
            TimelineItemMessage::None => TimelineItemAction::None,
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, TimelineItemMessage> {
        let fallback = w::text(format!("{:?}", self)).into();

        match &self.kind {
            TimelineItemKind::DateDivider(_) => fallback,
            TimelineItemKind::FailedToParseMessageLike { .. } => fallback,
            TimelineItemKind::FailedToParseState { .. } => fallback,
            TimelineItemKind::ReadMarker => fallback,
            TimelineItemKind::TimelineStart => fallback,
            TimelineItemKind::System(_) => fallback,
            TimelineItemKind::Message(msg) => {
                msg.view(theme, structure, &self.avatar_cache, &self.thumbnail_cache)
            }
        }
    }
}

#[derive(Debug, Clone)]
enum TimelineItemKind {
    DateDivider(SystemTime),
    TimelineStart,
    ReadMarker,
    Message(Box<MessageEvent>),
    System(Arc<SystemEvent>),
    FailedToParseMessageLike {
        event_type: String,
        error: Arc<serde_json::Error>,
    },
    FailedToParseState {
        event_type: String,
        state_key: String,
        error: Arc<serde_json::Error>,
    },
}

impl Hash for TimelineItemKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Other things are static, so hashing doesn't need to include them
        if let TimelineItemKind::Message(msg) = self {
            msg.hash(state);
        }
    }
}

#[derive(Debug)]
pub struct TimelineProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<OwnedMxcUri>,
    user_id: OwnedUserId,
}

impl ProfileLike for TimelineProfile {
    type Id<'a>
        = &'a UserId
    where
        Self: 'a;

    const ICON_BORDER_RADIUS_RATIO: f32 = 0.5;

    fn profile_name(&self) -> Option<String> {
        self.display_name.clone()
    }

    fn profile_avatar(&self) -> Option<OwnedMxcUri> {
        self.avatar_url.clone()
    }

    fn profile_id(&self) -> Self::Id<'_> {
        &self.user_id
    }
}

#[iced_cache(Debug, Clone)]
struct MessageEvent {
    timestamp: SystemTime,

    event_id: Option<OwnedEventId>,
    sender: OwnedUserId,
    sender_profile: TimelineDetails<Arc<TimelineProfile>>,

    in_reply_to: Arc<Vec<ReplyToDetails>>,

    reactions: ReactionsByKeyBySender,

    is_own: bool,
    is_editable: bool,
    is_highlighted: bool,
    can_be_replied_to: bool,
    contains_only_emojis: bool,

    shield: TimelineEventShieldState,

    content: MsgLikeKind,

    send_state: Option<EventSendState>,
}

impl MessageEvent {
    fn is_local_echo(&self) -> bool {
        !matches!(self.send_state, Some(EventSendState::Sent { .. }))
    }
}

#[derive(Debug)]
struct ReplyToDetails {
    event_id: OwnedEventId,
    event: TimelineDetails<ReplyEvent>,
}

#[derive(Debug)]
struct ReplyEvent {
    sender: OwnedUserId,
    sender_profile: TimelineDetails<Profile>,
    content: ReplyContent,
}

#[derive(Debug)]
enum ReplyContent {
    Audio,
    Text(String),
    System(String),
    Error(String),
    Emote(String),
    Media,
    Location,
    Poll,
    Redacted,
    Sticker,
    ProfileChange,
    RtcNotification(String),
    CallInvite,
}

#[iced_cache(Debug)]
struct SystemEvent {
    timestamp: SystemTime,

    event_id: Option<OwnedEventId>,
    sender: OwnedUserId,
    sender_profile: TimelineDetails<Arc<TimelineProfile>>,

    content: SystemMessage,
}

#[derive(Debug)]
enum SystemMessage {
    MemberhipChange(RoomMembershipChange),
    ProfileChange(MemberProfileChange),
    OtherState(OtherState),
    CallInvite,
    RtcNotification {
        call_intent: Option<CallIntent>,
        declined_by: Vec<OwnedUserId>,
    },
}
