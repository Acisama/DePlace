use std::sync::Arc;

use gpui::{ElementId, SharedString};
use matrix_sdk::ruma::{OwnedEventId, OwnedUserId, events::room::MediaSource};

mod convert;
mod render;

pub use convert::cached_from_timeline_item;

#[derive(Clone)]
pub struct CachedTimelineItem {
    kind: CachedTimelineItemKind,
    id: SharedString,
}

impl CachedTimelineItem {
    pub fn id(&self) -> ElementId {
        ElementId::Name(self.id.clone())
    }
}

#[derive(Clone)]
enum CachedTimelineItemKind {
    DateDivider(SharedString),
    ReadMarker,
    TimelineStart,
    Event(CachedTimelineEvent),
}

#[derive(Clone)]
struct CachedProgress {
    pub current: usize,
    pub total: usize,
}

#[derive(Clone)]
struct CachedMediaUploadProgress {
    pub index: u64,
    pub progress: CachedProgress,
}

#[derive(Clone)]
enum CachedSendState {
    NotSentYet {
        progress: Option<CachedMediaUploadProgress>,
    },
    SendingFailed {
        error: SharedString,
        is_recoverable: bool,
    },
    Sent {
        event_id: Arc<OwnedEventId>,
    },
}

#[derive(Clone)]
struct EventFlags {
    is_editable: bool,
    is_deletable: bool,
    is_reactable: bool,
    is_highlighted: bool,
    can_be_replied_to: bool,
    contains_only_emojis: bool,
}

#[derive(Clone)]
struct CachedTimelineEvent {
    state: Option<CachedSendState>,
    flags: EventFlags,

    short_time: SharedString,
    long_time: SharedString,

    timestamp: i64,
    sender: Arc<OwnedUserId>,
    read_by: Vec<Arc<OwnedUserId>>,

    event_id: Option<Arc<OwnedEventId>>,

    content: CachedEventContent,
}

impl CachedTimelineEvent {
    pub fn calculate_flags(&mut self, is_own: bool, is_redacted: bool) {
        let can_be_replied_to = self.flags.can_be_replied_to
            && match &self.content {
                CachedEventContent::UserMessage(content) => matches!(
                    &content.msg_type,
                    CachedMessageType::Text
                        | CachedMessageType::Emote
                        | CachedMessageType::Notice
                        | CachedMessageType::Poll { .. }
                        | CachedMessageType::Audio { .. }
                        | CachedMessageType::File { .. }
                        | CachedMessageType::Image { .. }
                        | CachedMessageType::LiveLocation { .. }
                        | CachedMessageType::Location(_)
                        | CachedMessageType::Sticker { .. }
                        | CachedMessageType::Video { .. }
                        | CachedMessageType::UnableToDecrypt
                ),
                _ => false,
            };

        let is_editable = is_own
            && self.flags.is_editable
            && match &self.content {
                CachedEventContent::UserMessage(content) => matches!(
                    &content.msg_type,
                    CachedMessageType::Text
                        | CachedMessageType::Emote
                        | CachedMessageType::Notice
                        | CachedMessageType::Poll { .. }
                ),
                _ => false,
            };

        let is_reactable = match &self.content {
            CachedEventContent::UserMessage(content) if is_redacted => matches!(
                &content.msg_type,
                CachedMessageType::Text
                    | CachedMessageType::Emote
                    | CachedMessageType::Notice
                    | CachedMessageType::Poll { .. }
                    | CachedMessageType::Audio { .. }
                    | CachedMessageType::File { .. }
                    | CachedMessageType::Image { .. }
                    | CachedMessageType::LiveLocation { .. }
                    | CachedMessageType::Location(_)
                    | CachedMessageType::Sticker { .. }
                    | CachedMessageType::Video { .. }
            ),
            _ => false,
        };

        self.flags.can_be_replied_to = can_be_replied_to;
        self.flags.is_editable = is_editable;
        self.flags.is_reactable = is_reactable;
        self.flags.is_deletable = is_own;
    }
}

#[derive(Clone)]
pub enum DetailState<T> {
    Unavailable,
    Pending,
    Ready(T),
    Error(SharedString),
}

#[derive(Clone)]
struct CachedReplyInfo {
    event_id: Arc<OwnedEventId>,
    body: DetailState<SharedString>,
}

#[derive(Clone)]
enum CachedEventContent {
    SystemMessage(CachedSystemMessage),
    UserMessage(CachedUserMessage),
    FailedToParseMessageLike(SharedString),
    FailedToParseState(SharedString),
}

#[derive(Clone)]
struct ReactionInfo {
    reactors: Vec<(Arc<OwnedUserId>, u64)>,
    emoji: SharedString,
    timestamp: u64,
    has_own: bool,
}

#[derive(Clone)]
struct CachedUserMessage {
    reactions: Vec<ReactionInfo>,
    in_reply_to: Option<CachedReplyInfo>,

    is_edited: bool,
    body: Option<SharedString>,

    msg_type: CachedMessageType,
}

#[derive(Clone)]
struct CachedBeaconInfo {
    pub geo_uri: SharedString,
    pub description: Option<SharedString>,
    pub timestamp: i64,
}

#[derive(Clone)]
enum CachedMessageType {
    Audio {
        source: Arc<MediaSource>,
        filename: SharedString,
        duration: Option<u64>,
    },
    Emote,
    Empty,
    File {
        source: Arc<MediaSource>,
        filename: SharedString,
        mime_type: Option<SharedString>,
        size: Option<u64>,
    },
    Image {
        filename: SharedString,
        source: Arc<MediaSource>,
        width: Option<u64>,
        height: Option<u64>,
        size: Option<u64>,
        mime_type: Option<SharedString>,
        blurhash: Option<SharedString>,
    },
    Location(CachedBeaconInfo),
    Notice,
    ServerNotice {
        admin_contact: Option<SharedString>,
    },
    Text,
    VerificationRequest,
    Video {
        source: Arc<MediaSource>,
        filename: SharedString,
        width: Option<u64>,
        height: Option<u64>,
        size: Option<u64>,
        duration: Option<u64>,
        mime_type: Option<SharedString>,
        blurhash: Option<SharedString>,
    },
    LiveLocation {
        locations: Vec<CachedBeaconInfo>,
    },
    Poll,
    UnableToDecrypt,
    Redacted,
    Sticker,
    Other(SharedString),
}

#[derive(Clone)]
enum CachedSystemMessage {
    CallInvite(SharedString),
    MemberShipChange(SharedString),
    ProfileChange(SharedString),
    RtcNotification {
        text: SharedString,
        declined_by: Vec<Arc<OwnedUserId>>,
    },
    PolicyRuleRoom(SharedString),
    PolicyRuleServer(SharedString),
    PolicyRuleUser(SharedString),
    RoomAvatar(SharedString),
    RoomCanonicalAlias(SharedString),
    RoomCreate(SharedString),
    RoomEncryption(SharedString),
    RoomGuestAccess(SharedString),
    RoomHistoryVisibility(SharedString),
    RoomJoinRules(SharedString),
    RoomName(SharedString),
    RoomPinnedEvents(SharedString),
    RoomPowerLevels(SharedString),
    RoomServerAcl(SharedString),
    RoomThirdPartyInvite(SharedString),
    RoomTombstone(SharedString),
    RoomTopic(SharedString),
    SpaceChild(SharedString),
    SpaceParent(SharedString),
    Unknown(SharedString),
    Invisible,
}

impl CachedSystemMessage {
    pub fn text(&self) -> Option<SharedString> {
        match self {
            CachedSystemMessage::CallInvite(text)
            | CachedSystemMessage::MemberShipChange(text)
            | CachedSystemMessage::ProfileChange(text)
            | CachedSystemMessage::RtcNotification { text, .. }
            | CachedSystemMessage::PolicyRuleRoom(text)
            | CachedSystemMessage::PolicyRuleServer(text)
            | CachedSystemMessage::PolicyRuleUser(text)
            | CachedSystemMessage::RoomAvatar(text)
            | CachedSystemMessage::RoomCanonicalAlias(text)
            | CachedSystemMessage::RoomCreate(text)
            | CachedSystemMessage::RoomEncryption(text)
            | CachedSystemMessage::RoomGuestAccess(text)
            | CachedSystemMessage::RoomHistoryVisibility(text)
            | CachedSystemMessage::RoomJoinRules(text)
            | CachedSystemMessage::RoomName(text)
            | CachedSystemMessage::RoomPinnedEvents(text)
            | CachedSystemMessage::RoomPowerLevels(text)
            | CachedSystemMessage::RoomServerAcl(text)
            | CachedSystemMessage::RoomThirdPartyInvite(text)
            | CachedSystemMessage::RoomTombstone(text)
            | CachedSystemMessage::RoomTopic(text)
            | CachedSystemMessage::SpaceChild(text)
            | CachedSystemMessage::SpaceParent(text)
            | CachedSystemMessage::Unknown(text) => Some(text.clone()),
            CachedSystemMessage::Invisible => None,
        }
    }
}
