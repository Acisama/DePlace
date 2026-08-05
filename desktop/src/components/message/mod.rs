use std::sync::Arc;

use gpui::{ElementId, ImageFormat, SharedString, StyleRefinement, Styled};
use matrix_sdk::ruma::{OwnedEventId, OwnedUserId, events::room::MediaSource};

mod convert;
mod render;

pub use convert::cached_from_timeline_item;

use crate::{
    components::ByteSize,
    theme::{AppTheme, Structure},
};

#[derive(Clone)]
pub struct CachedTimelineItem {
    pub kind: CachedTimelineItemKind,
    id: SharedString,
}

impl CachedTimelineItem {
    pub fn is_user_message(&self) -> bool {
        match &self.kind {
            CachedTimelineItemKind::Event(event) => event.content.is_user_message(),
            _ => false,
        }
    }
}

impl CachedTimelineItem {
    pub fn id(&self) -> ElementId {
        ElementId::Name(self.id.clone())
    }
}

impl CachedTimelineItem {
    /// Recomputes whether this item should show its own header (avatar/name/time),
    /// based on the identity of the item immediately before it.
    pub fn recompute_show_header(&mut self, prev: Option<&CachedTimelineItem>) {
        let CachedTimelineItemKind::Event(event) = &self.kind else {
            return;
        };

        let show_header = event.in_reply_to().is_some()
            || prev
                .map(|item| {
                    if let CachedTimelineItemKind::Event(prev_event) = &item.kind {
                        prev_event.timestamp.abs_diff(event.timestamp) > 300
                            || prev_event.sender != event.sender
                    } else {
                        true
                    }
                })
                .unwrap_or(true);

        let CachedTimelineItemKind::Event(event) = &mut self.kind else {
            return;
        };
        event.show_header = show_header;
    }

    /// Recomputes whether this item should pad its bottom margin (i.e. the next
    /// item starts a new visual group), based on the identity of the item
    /// immediately after it.
    pub fn recompute_pad_bottom(&mut self, next: Option<&CachedTimelineItem>) {
        let CachedTimelineItemKind::Event(event) = &self.kind else {
            return;
        };

        let pad_bottom = next.is_some_and(|item| {
            if let CachedTimelineItemKind::Event(next_event) = &item.kind {
                next_event.timestamp.abs_diff(event.timestamp) > 300
                    || next_event.sender != event.sender
            } else {
                true
            }
        });

        let CachedTimelineItemKind::Event(event) = &mut self.kind else {
            return;
        };
        event.pad_bottom = pad_bottom;
    }
}

#[derive(Clone)]
pub enum CachedTimelineItemKind {
    DateDivider(SharedString),
    ReadMarker,
    TimelineStart,
    Event(Box<CachedTimelineEvent>),
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
pub struct CachedTimelineEvent {
    state: Option<CachedSendState>,
    flags: EventFlags,

    short_time: SharedString,
    long_time: SharedString,

    timestamp: i64,
    sender: Arc<OwnedUserId>,
    read_by: Vec<Arc<OwnedUserId>>,

    show_header: bool,
    pad_bottom: bool,

    event_id: Option<Arc<OwnedEventId>>,

    content: CachedEventContent,
}

impl CachedTimelineEvent {
    pub fn in_reply_to(&self) -> Option<CachedReplyInfo> {
        match &self.content {
            CachedEventContent::UserMessage(content) => content.in_reply_to.clone(),
            _ => None,
        }
    }

    pub fn calculate_flags(&mut self, is_own: bool, is_redacted: bool) {
        let can_be_replied_to = self.flags.can_be_replied_to
            && match &self.content {
                CachedEventContent::UserMessage(content) => matches!(
                    &content.msg_type,
                    CachedMessageType::Text
                        | CachedMessageType::Emote
                        | CachedMessageType::Notice
                        | CachedMessageType::Poll
                        | CachedMessageType::Audio { .. }
                        | CachedMessageType::File { .. }
                        | CachedMessageType::Image { .. }
                        | CachedMessageType::LiveLocation { .. }
                        | CachedMessageType::Location(_)
                        | CachedMessageType::Sticker
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
                        | CachedMessageType::Poll
                ),
                _ => false,
            };

        let is_reactable = match &self.content {
            CachedEventContent::UserMessage(content) if is_redacted => matches!(
                &content.msg_type,
                CachedMessageType::Text
                    | CachedMessageType::Emote
                    | CachedMessageType::Notice
                    | CachedMessageType::Poll
                    | CachedMessageType::Audio { .. }
                    | CachedMessageType::File { .. }
                    | CachedMessageType::Image { .. }
                    | CachedMessageType::LiveLocation { .. }
                    | CachedMessageType::Location(_)
                    | CachedMessageType::Sticker
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
pub enum CachedReplyPreviewBody {
    Audio,
    Text(SharedString),
    System(SharedString),
    Error(SharedString),
    Emote(SharedString),
    Media,
    Location,
    Poll,
    Redacted,
    Sticker,
    ProfileChange,
    RtcNotification(SharedString),
    CallInvite,
}

impl CachedReplyPreviewBody {
    pub fn render_things(
        &self,
        theme: &AppTheme,
        structure: &Structure,
    ) -> (Option<&'static str>, Option<SharedString>, StyleRefinement) {
        match self {
            CachedReplyPreviewBody::Audio => (
                Some(phosphor_svgs::icon::music_note::BOLD),
                Some("Audio message".into()),
                StyleRefinement::default(),
            ),
            CachedReplyPreviewBody::Emote(text) => (
                None,
                Some(text.clone()),
                StyleRefinement::default().text_size(structure.chat.text_size * 1.5),
            ),
            CachedReplyPreviewBody::Text(text) => {
                (None, Some(text.clone()), StyleRefinement::default())
            }
            CachedReplyPreviewBody::System(text) => (
                None,
                Some(text.clone()),
                StyleRefinement::default()
                    .text_color(theme.text.dim)
                    .italic(),
            ),
            CachedReplyPreviewBody::Redacted => (
                Some(phosphor_svgs::icon::trash::BOLD),
                Some("Redacted".into()),
                StyleRefinement::default()
                    .text_color(theme.text.dim)
                    .italic(),
            ),
            CachedReplyPreviewBody::Error(text) => (
                Some(phosphor_svgs::icon::warning::BOLD),
                Some(text.clone()),
                StyleRefinement::default().text_color(theme.colors.error),
            ),
            CachedReplyPreviewBody::Media => (
                Some(phosphor_svgs::icon::image::BOLD),
                Some("Click to see media".into()),
                StyleRefinement::default().italic(),
            ),
            CachedReplyPreviewBody::Location => (
                Some(phosphor_svgs::icon::map_pin::BOLD),
                Some("Click to see location".into()),
                StyleRefinement::default().italic(),
            ),
            CachedReplyPreviewBody::Poll => (
                Some(phosphor_svgs::icon::clipboard_text::BOLD),
                Some("Click to see poll".into()),
                StyleRefinement::default().italic(),
            ),
            CachedReplyPreviewBody::Sticker => (
                Some(phosphor_svgs::icon::sticker::BOLD),
                Some("Click to see sticker".into()),
                StyleRefinement::default().italic(),
            ),
            CachedReplyPreviewBody::ProfileChange => (
                Some(phosphor_svgs::icon::user::BOLD),
                Some("Click to see profile change".into()),
                StyleRefinement::default().text_color(theme.text.dim),
            ),
            CachedReplyPreviewBody::RtcNotification(text) => (
                None,
                Some(text.clone()),
                StyleRefinement::default().text_color(theme.text.dim),
            ),
            CachedReplyPreviewBody::CallInvite => (
                Some(phosphor_svgs::icon::phone::BOLD),
                Some("Click to see call".into()),
                StyleRefinement::default().text_color(theme.text.dim),
            ),
        }
    }
}

#[derive(Clone)]
pub struct CachedReplyPreview {
    pub sender_id: Arc<OwnedUserId>,
    pub body: CachedReplyPreviewBody,
}

#[derive(Clone)]
pub struct CachedReplyInfo {
    pub event_id: Arc<OwnedEventId>,
    pub body: DetailState<CachedReplyPreview>,
}

#[derive(Clone)]
enum CachedEventContent {
    SystemMessage(CachedSystemMessage),
    UserMessage(Box<CachedUserMessage>),
    FailedToParseMessageLike(SharedString),
    FailedToParseState(SharedString),
}

impl CachedEventContent {
    #[inline]
    pub fn is_user_message(&self) -> bool {
        matches!(self, CachedEventContent::UserMessage(_))
    }
}

#[derive(Clone)]
struct ReactionInfo {
    reactors: Vec<Arc<OwnedUserId>>,
    reactors_count: SharedString,
    emoji: SharedString,
    timestamp: u64,
    has_own: bool,
}

#[derive(Clone)]
struct CachedUserMessage {
    reactions: Option<Vec<ReactionInfo>>,
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
        size: Option<ByteSize>,
    },
    Image {
        filename: SharedString,
        source: Arc<MediaSource>,
        source_key: SharedString,
        width: Option<f32>,
        height: Option<f32>,
        size: Option<ByteSize>,
        format: Option<ImageFormat>,
        blurhash_image: Option<Arc<gpui::RenderImage>>,
    },
    Location(CachedBeaconInfo),
    Notice,
    ServerNotice {
        admin_contact: Option<SharedString>,
    },
    Text,
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
    Other {
        msg_type: SharedString,
    },
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
