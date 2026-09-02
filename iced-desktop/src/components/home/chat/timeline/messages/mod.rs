use crate::common::*;
use std::{collections::BTreeSet, time::SystemTime};

use matrix_sdk::ruma::events::rtc::notification::CallIntent;
use matrix_sdk_ui::timeline::{
    EventSendState, MemberProfileChange, MsgLikeKind, OtherState, Profile, ReactionsByKeyBySender,
    RoomMembershipChange, TimelineDetails, TimelineEventShieldState,
};

mod convert;
mod hashing;
mod render;

pub use convert::ToTimelineItem;

#[derive(Debug)]
pub struct TimelineItem {
    pub id: String,
    kind: TimelineItemKind,
}

#[derive(Debug)]
enum TimelineItemKind {
    DateDivider(SystemTime),
    TimelineStart,
    ReadMarker,
    Message(Box<MessageEvent>),
    System(Box<SystemEvent>),
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

#[derive(Debug)]
struct MessageEvent {
    timestamp: SystemTime,

    event_id: Option<OwnedEventId>,
    sender: OwnedUserId,
    sender_profile: TimelineDetails<Profile>,

    in_reply_to: Vec<ReplyToDetails>,

    reactions: ReactionsByKeyBySender,

    is_own: bool,
    is_editable: bool,
    is_highlighted: bool,
    can_be_replied_to: bool,
    contains_only_emojis: bool,

    shield: TimelineEventShieldState,

    avatar_cache: AvatarCache,
    relevant_hash_urls: BTreeSet<OwnedMxcUri>,

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

#[derive(Debug)]
struct SystemEvent {
    timestamp: SystemTime,

    event_id: Option<OwnedEventId>,
    sender: OwnedUserId,
    sender_profile: TimelineDetails<Profile>,

    avatar_cache: AvatarCache,
    relevant_hash_urls: BTreeSet<OwnedMxcUri>,

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
