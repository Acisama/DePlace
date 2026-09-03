use crate::common::*;
use std::time::SystemTime;

use deplace_core::formatting::format_date_divider;
use iced::Alignment;
use macros::iced_cache;
use matrix_sdk::{
    media::UniqueKey,
    ruma::{
        OwnedRoomAliasId,
        events::{
            room::{
                MediaSource,
                guest_access::GuestAccess,
                history_visibility::HistoryVisibility,
                message::{FileInfo, FormattedBody, UrlPreview},
            },
            rtc::notification::CallIntent,
        },
        room::JoinRule,
    },
};
use matrix_sdk_ui::timeline::{
    EventSendState, MemberProfileChange, Profile, ReactionsByKeyBySender, RoomMembershipChange,
    TimelineDetails, TimelineEventShieldState,
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
    EventEnter,
    EventExit,
    MediaMouseEnter,
    MediaMouseLeave,
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
    Update,
}

#[iced_cache(Debug, Clone)]
pub struct TimelineItem {
    #[hash]
    pub id: String,
    #[hash]
    kind: TimelineItemKind,

    #[hash]
    is_hovered: bool,

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
            TimelineItemMessage::MediaMouseEnter
                if let TimelineItemKind::Message(msg) = &mut self.kind =>
            {
                msg.media_hovered = true;
                TimelineItemAction::Update
            }
            TimelineItemMessage::MediaMouseLeave
                if let TimelineItemKind::Message(msg) = &mut self.kind =>
            {
                msg.media_hovered = false;
                TimelineItemAction::Update
            }
            TimelineItemMessage::EventEnter => {
                self.is_hovered = true;
                TimelineItemAction::Update
            }
            TimelineItemMessage::EventExit => {
                self.is_hovered = false;
                TimelineItemAction::Update
            }
            _ => TimelineItemAction::None,
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, TimelineItemMessage> {
        let fallback = w::text(format!("{:?}", self)).into();

        match &self.kind {
            TimelineItemKind::DateDivider(date) => w::row![
                w::container("")
                    .width(Fill)
                    .height(structure.divider_width)
                    .style(move |_| ContainerStyle::default()
                        .background(theme.border)
                        .border(border::rounded(structure.divider_width / 2.0))),
                w::text(format_date_divider(*date, chrono_tz::Tz::UTC)).color(theme.text.dim),
                w::container("")
                    .width(Fill)
                    .height(structure.divider_width)
                    .style(move |_| ContainerStyle::default()
                        .background(theme.border)
                        .border(border::rounded(structure.divider_width / 2.0))),
            ]
            .align_y(Alignment::Center)
            .spacing(structure.small_gap / 2.0)
            .into(),
            TimelineItemKind::FailedToParseMessageLike { .. } => fallback,
            TimelineItemKind::FailedToParseState { .. } => fallback,
            TimelineItemKind::ReadMarker => w::container("")
                .width(Fill)
                .height(structure.divider_width)
                .style(move |_| ContainerStyle::default().background(theme.accent))
                .into(),
            TimelineItemKind::TimelineStart => fallback,
            TimelineItemKind::System(sys) => {
                sys.view(theme, structure, &self.avatar_cache, self.is_hovered)
            }
            TimelineItemKind::Message(msg) => msg.view(
                theme,
                structure,
                &self.avatar_cache,
                &self.thumbnail_cache,
                self.is_hovered,
            ),
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
        event_type: Arc<String>,
        error: Arc<serde_json::Error>,
    },
    FailedToParseState {
        event_type: Arc<String>,
        state_key: Arc<String>,
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

    #[hash]
    media_hovered: bool,

    is_own: bool,
    is_editable: bool,
    is_highlighted: bool,
    can_be_replied_to: bool,
    contains_only_emojis: bool,

    shield: TimelineEventShieldState,

    #[hash]
    content: Arc<MessageContent>,

    send_state: Option<EventSendState>,
}

impl MessageEvent {
    fn is_local_echo(&self) -> bool {
        !matches!(self.send_state, Some(EventSendState::Sent { .. }))
    }
}

#[derive(Debug, Clone)]
pub struct VisualInfo {
    width: Option<u64>,
    height: Option<u64>,
    size: Option<u64>,
    thumbnail_source: Option<MediaSource>,
}

impl Hash for VisualInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.width.hash(state);
        self.height.hash(state);
        self.size.hash(state);
        self.thumbnail_source
            .as_ref()
            .map(|s| s.unique_key())
            .hash(state);
    }
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Audio,
    Emote {
        body: Option<String>,
        formatted_body: Option<FormattedBody>,
    },
    Empty,
    File {
        caption: Option<String>,
        formatted_caption: Option<FormattedBody>,
        filename: String,
        source: MediaSource,
        info: Option<Box<FileInfo>>,
    },
    Image {
        caption: Option<String>,
        formatted_caption: Option<FormattedBody>,

        blur_preview: Option<ImageHandle>,

        filename: String,
        source: MediaSource,
        info: Option<VisualInfo>,
    },
    Location,
    Notice {
        body: Option<String>,
        formatted_body: Option<FormattedBody>,
    },
    ServerNotice {
        body: Option<String>,
    },
    Text {
        body: Option<String>,
        formatted_body: Option<FormattedBody>,
        _url_previews: Option<Vec<UrlPreview>>,
    },
    Video {
        caption: Option<String>,
        formatted_caption: Option<FormattedBody>,

        blur_preview: Option<ImageHandle>,

        filename: String,
        source: MediaSource,
        info: Option<VisualInfo>,
    },
    /// Body is only present if the client doesn't support the key verification framework, this client doesn't support it
    VerificationRequest {
        body: Option<String>,
        formatted_body: Option<FormattedBody>,
    },
    Sticker,
    Poll,
    Redacted,
    UnableToDecrypt,
    Other {
        event_type: Arc<String>,
    },
    LiveLocation,
}

impl std::hash::Hash for MessageContent {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);

        match self {
            MessageContent::Emote {
                body,
                formatted_body,
            } => {
                body.hash(state);
                formatted_body.as_ref().map(|c| &c.body).hash(state);
            }
            MessageContent::File {
                caption,
                formatted_caption,
                filename,
                source,
                info,
            } => {
                caption.hash(state);
                formatted_caption.as_ref().map(|c| &c.body).hash(state);
                filename.hash(state);
                source.unique_key().hash(state);
                if let Some(info) = info {
                    info.mimetype.hash(state);
                    info.size.hash(state);
                    info.thumbnail_source
                        .as_ref()
                        .map(UniqueKey::unique_key)
                        .hash(state);
                }
            }
            MessageContent::Image {
                caption,
                formatted_caption,
                filename,
                source,
                info,
                ..
            } => {
                caption.hash(state);
                formatted_caption.as_ref().map(|c| &c.body).hash(state);
                filename.hash(state);
                source.unique_key().hash(state);
                info.hash(state);
            }
            MessageContent::Notice {
                body,
                formatted_body,
            } => {
                body.hash(state);
                formatted_body.as_ref().map(|c| &c.body).hash(state);
            }
            MessageContent::ServerNotice { body } => {
                body.hash(state);
            }
            MessageContent::Text {
                body,
                formatted_body,
                ..
            } => {
                body.hash(state);
                formatted_body.as_ref().map(|c| &c.body).hash(state);
            }
            MessageContent::Video {
                caption,
                formatted_caption,
                filename,
                source,
                info,
                ..
            } => {
                caption.hash(state);
                formatted_caption.as_ref().map(|c| &c.body).hash(state);
                filename.hash(state);
                source.unique_key().hash(state);
                info.hash(state);
            }
            MessageContent::Other { event_type } => event_type.hash(state),
            _ => {}
        }
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
    MemberhipChange(Box<RoomMembershipChange>),
    ProfileChange(Box<MemberProfileChange>),
    CallInvite,
    RtcNotification {
        call_intent: Option<CallIntent>,
        declined_by: Vec<OwnedUserId>,
    },
    Custom {
        event_type: String,
    },

    PolicyRuleRoom,
    PolicyRuleServer,
    PolicyRuleUser,
    RoomAvatar(EventChange<OwnedMxcUri>),
    RoomCanonicalAlias(EventChange<OwnedRoomAliasId>),
    RoomCreate,
    RoomEncryption,
    RoomGuestAccess(EventChange<GuestAccess>),
    RoomHistoryVisibility(EventChange<HistoryVisibility>),
    RoomJoinRules(EventChange<JoinRule>),
    RoomName(EventChange<String>),
    RoomPinnedEvents,
    RoomPowerLevels,
    RoomServerAcl,
    RoomThirdPartyInvite,
    RoomTombstone,
    RoomTopic(EventChange<String>),
    SpaceChild,
    SpaceParent,
}
