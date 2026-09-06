use crate::common::*;
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime},
};

use deplace_core::{formatting::format_date_divider, state::cache::VideoCache};
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
                message::{FileInfo, UrlPreview},
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
use render::render_event;

/// The message an item in the timeline can receive
#[derive(Debug, Clone)]
pub enum TimelineItemMessage {
    NeedsMedia(NeedsMedia),
    EventEnter,
    EventExit,
    MediaMouseEnter,
    MediaMouseLeave,
    ToggleVideoPause(Arc<iced_video_player::Video>),
}

impl NeedsAvatarExt for TimelineItemMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        TimelineItemMessage::NeedsMedia(NeedsMedia::Avatar { uri })
    }
}

pub enum TimelineItemAction {
    Update,
    NeedsMedia(NeedsMedia),
}

/// An item in the timeline
///
/// This is the most abstract version, able to represent anything
/// in the timeline.
#[iced_cache(Debug, Clone)]
pub struct TimelineItem {
    #[hash]
    pub id: String,
    #[hash]
    room_id: OwnedRoomId,
    #[hash]
    kind: TimelineItemKind,
}

impl TimelineItem {
    /// Computes what `connects_before` should be for this
    /// item given its neighbors. Pure, so the item can only be mutated in the list if necessary.
    pub fn compute_connection(&self, previous: Option<&TimelineItem>) -> Option<bool> {
        let TimelineItemKind::Message { event, .. } = &self.kind else {
            return None;
        };
        let connection_duration = Duration::from_mins(5);

        let connects_before = if !event.in_reply_to.is_empty() {
            false
        } else if let Some(TimelineItemKind::Message {
            event: previous_event,
            ..
        }) = previous.map(|p| &p.kind)
        {
            event.sender == previous_event.sender
                && event
                    .timestamp
                    .duration_since(previous_event.timestamp)
                    .map(|res| res < connection_duration)
                    .unwrap_or(false)
        } else {
            false
        };

        if connects_before == event.connects_previous {
            return None;
        }

        Some(connects_before)
    }

    pub fn compute_prev_is_event(&self, previous: Option<&TimelineItem>) -> Option<bool> {
        let previous_is_event = match &self.kind {
            TimelineItemKind::Message {
                previous_is_event, ..
            } => *previous_is_event,
            TimelineItemKind::System {
                previous_is_event, ..
            } => *previous_is_event,
            _ => return None,
        };

        let prev_is_event = matches!(
            previous.map(|p| &p.kind),
            Some(TimelineItemKind::Message { .. }) | Some(TimelineItemKind::System { .. })
        );

        if prev_is_event == previous_is_event {
            return None;
        }

        Some(prev_is_event)
    }

    pub fn set_connection(&mut self, connects_before: bool) {
        if let TimelineItemKind::Message { event, .. } = &mut self.kind {
            event.connects_previous = connects_before;
        }
    }

    pub fn set_previous_is_event(&mut self, previous_is_event: bool) {
        match &mut self.kind {
            TimelineItemKind::Message {
                previous_is_event: prev_is_event,
                ..
            } => {
                *prev_is_event = previous_is_event;
            }
            TimelineItemKind::System {
                previous_is_event: prev_is_event,
                ..
            } => {
                *prev_is_event = previous_is_event;
            }
            _ => {}
        }
    }

    fn avatar_hashes(&mut self) -> Option<&mut BTreeSet<OwnedMxcUri>> {
        if let TimelineItemKind::Message { event, .. } = &mut self.kind {
            Some(&mut event.avatar_states_for_hash)
        } else {
            None
        }
    }

    fn thumbnail_hashes(&mut self) -> Option<&mut BTreeSet<(String, u64, u64)>> {
        if let TimelineItemKind::Message { event, .. } = &mut self.kind
            && let MessageContent::Image { image, .. } = &mut event.content
        {
            Some(&mut image.thumbnail_states_for_hash)
        } else {
            None
        }
    }

    fn video_hashes(&mut self) -> Option<&mut BTreeSet<String>> {
        if let TimelineItemKind::Message { event, .. } = &mut self.kind
            && let MessageContent::Video { video, .. } = &mut event.content
        {
            Some(&mut video.video_states_for_hash)
        } else {
            None
        }
    }

    fn set_hovered(&mut self, new_hovered: bool) -> Option<TimelineItemAction> {
        match &mut self.kind {
            TimelineItemKind::Message { is_hovered, .. } if *is_hovered != new_hovered => {
                *is_hovered = new_hovered;
                return Some(TimelineItemAction::Update);
            }
            TimelineItemKind::System { is_hovered, .. } if *is_hovered != new_hovered => {
                *is_hovered = new_hovered;
                return Some(TimelineItemAction::Update);
            }
            _ => {}
        }
        None
    }
}

impl IcedWidget<TimelineItemMessage, TimelineItemAction> for TimelineItem {
    fn update(&mut self, message: TimelineItemMessage) -> Option<TimelineItemAction> {
        match message {
            TimelineItemMessage::MediaMouseEnter => {
                if let TimelineItemKind::Message { event, .. } = &mut self.kind {
                    match &mut event.content {
                        MessageContent::Image { is_hovered, .. } if !*is_hovered => {
                            *is_hovered = true;
                            Some(TimelineItemAction::Update)
                        }
                        MessageContent::Video { is_hovered, .. } if !*is_hovered => {
                            *is_hovered = true;
                            Some(TimelineItemAction::Update)
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
            TimelineItemMessage::MediaMouseLeave => {
                if let TimelineItemKind::Message { event, .. } = &mut self.kind {
                    match &mut event.content {
                        MessageContent::Image { is_hovered, .. } if *is_hovered => {
                            *is_hovered = false;
                            Some(TimelineItemAction::Update)
                        }
                        MessageContent::Video { is_hovered, .. } if *is_hovered => {
                            *is_hovered = false;
                            Some(TimelineItemAction::Update)
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
            TimelineItemMessage::EventEnter => self.set_hovered(true),
            TimelineItemMessage::EventExit => self.set_hovered(false),
            TimelineItemMessage::NeedsMedia(media) => {
                match &media {
                    NeedsMedia::Avatar { uri } if let Some(hashes) = self.avatar_hashes() => {
                        hashes.insert(uri.clone())
                    }
                    NeedsMedia::Thumbnail { key, .. }
                        if let Some(hashes) = self.thumbnail_hashes() =>
                    {
                        hashes.insert(key.clone())
                    }
                    NeedsMedia::Video { source, .. } if let Some(hashes) = self.video_hashes() => {
                        hashes.insert(source.unique_key())
                    }
                    _ => return None,
                };
                Some(TimelineItemAction::NeedsMedia(media))
            }
            TimelineItemMessage::ToggleVideoPause(video) => {
                video.set_paused(!video.paused());
                None
            }
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
            .spacing(structure.small_gap)
            .into(),
            TimelineItemKind::FailedToParseMessageLike { .. } => fallback,
            TimelineItemKind::FailedToParseState { .. } => fallback,
            TimelineItemKind::ReadMarker => w::container(
                w::container(Space::new())
                    .width(Fill)
                    .height(structure.divider_width)
                    .style(move |_| ContainerStyle::default().background(theme.accent)),
            )
            .padding(padding::vertical(structure.small_gap))
            .into(),
            TimelineItemKind::TimelineStart => fallback,
            TimelineItemKind::System {
                is_hovered,
                event,
                previous_is_event,
            } => render_event(
                w::lazy(event.clone(), move |event| event.view(theme, structure)),
                structure,
                theme,
                *is_hovered,
                *previous_is_event,
                false,
                structure.small_gap,
            ),
            TimelineItemKind::Message {
                event,
                is_hovered,
                previous_is_event,
            } => render_event(
                w::lazy(event.clone(), move |event| event.view(theme, structure)),
                structure,
                theme,
                *is_hovered,
                *previous_is_event,
                event.connects_previous,
                structure.gap * 1.5,
            ),
            _ => fallback,
        }
    }
}

#[derive(Debug, Clone)]
enum TimelineItemKind {
    DateDivider(SystemTime),
    TimelineStart,
    ReadMarker,
    Message {
        event: Box<MessageEvent>,
        is_hovered: bool,
        previous_is_event: bool,
    },
    System {
        is_hovered: bool,
        event: Box<SystemEvent>,
        previous_is_event: bool,
    },
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
        match self {
            TimelineItemKind::Message {
                event,
                is_hovered,
                previous_is_event: previous_is_text_message,
            } => {
                event.hash(state);
                is_hovered.hash(state);
                previous_is_text_message.hash(state);
            }
            TimelineItemKind::System {
                is_hovered,
                previous_is_event,
                ..
            } => {
                is_hovered.hash(state);
                previous_is_event.hash(state);
            }
            _ => {}
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

    reactions: Arc<ReactionsByKeyBySender>,

    /// Weather the previous message is the same type, sender and withing 5 minutes of this message.
    connects_previous: bool,

    avatar_cache: AvatarCache,

    is_own: bool,
    is_editable: bool,
    is_highlighted: bool,
    can_be_replied_to: bool,
    contains_only_emojis: bool,

    shield: TimelineEventShieldState,

    #[hash]
    content: MessageContent,

    send_state: Option<EventSendState>,
}

impl ExtraHash for MessageEvent {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self.sender_profile {
            TimelineDetails::Error(_) => 0.hash(state),
            TimelineDetails::Pending => 1.hash(state),
            TimelineDetails::Unavailable => 2.hash(state),
            TimelineDetails::Ready(_) => 3.hash(state),
        }

        match self.send_state {
            None => 0.hash(state),
            Some(EventSendState::NotSentYet { .. }) => 1.hash(state),
            Some(EventSendState::Sent { .. }) => 2.hash(state),
            Some(EventSendState::SendingFailed { .. }) => 3.hash(state),
        }
    }
}

impl MessageEvent {
    fn is_local_echo(&self) -> bool {
        !matches!(self.send_state, Some(EventSendState::Sent { .. }) | None)
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

#[derive(Debug, Clone, Hash)]
pub struct CustomBody {}

impl CustomBody {
    pub fn new() -> Self {
        CustomBody {}
    }
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Audio,
    Emote {
        body: Option<String>,
        formatted_body: Option<CustomBody>,
    },
    Empty,
    File {
        caption: Option<String>,
        formatted_caption: Option<CustomBody>,
        filename: String,
        source: MediaSource,
        info: Option<Box<FileInfo>>,
    },
    Image {
        is_hovered: bool,
        image: ImageMessage,
    },
    Location,
    Notice {
        body: Option<String>,
        formatted_body: Option<CustomBody>,
    },
    ServerNotice {
        body: Option<String>,
    },
    Text {
        body: Option<String>,
        formatted_body: Option<CustomBody>,
        _url_previews: Option<Vec<UrlPreview>>,
    },
    Video {
        is_hovered: bool,
        video: VideoMessage,
    },
    /// Body is only present if the client doesn't support the key verification framework, this client doesn't support it
    VerificationRequest {
        body: Option<String>,
        formatted_body: Option<CustomBody>,
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
                formatted_body.hash(state);
            }
            MessageContent::File {
                caption,
                formatted_caption,
                filename,
                source,
                info,
            } => {
                caption.hash(state);
                formatted_caption.hash(state);
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
            MessageContent::Image { is_hovered, image } => {
                is_hovered.hash(state);
                image.hash(state);
            }
            MessageContent::Notice {
                body,
                formatted_body,
            } => {
                body.hash(state);
                formatted_body.hash(state);
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
                formatted_body.hash(state);
            }
            MessageContent::Video { is_hovered, video } => {
                is_hovered.hash(state);
                video.hash(state);
            }
            MessageContent::Other { event_type } => event_type.hash(state),
            _ => {}
        }
    }
}
#[iced_cache(Debug, Clone)]
pub struct ImageMessage {
    #[hash]
    caption: Option<String>,
    #[hash]
    formatted_caption: Option<CustomBody>,

    blur_preview: Option<ImageHandle>,

    thumbnail_cache: ThumbnailCache,

    #[hash]
    filename: String,
    source: MediaSource,
    #[hash]
    info: Option<VisualInfo>,
}

impl ExtraHash for ImageMessage {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.unique_key().hash(state);
    }
}

#[iced_cache(Debug, Clone)]
pub struct VideoMessage {
    #[hash]
    caption: Option<String>,
    #[hash]
    formatted_caption: Option<CustomBody>,

    blur_preview: Option<ImageHandle>,

    video_cache: VideoCache,

    #[hash]
    filename: String,
    source: MediaSource,
    #[hash]
    info: Option<VisualInfo>,
}

impl ExtraHash for VideoMessage {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.unique_key().hash(state);
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
    sender_profile: TimelineDetails<TimelineProfile>,
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
    RtcNotification(String),
    CallInvite,
}

/// This has an empty hash implementation because it doesn't change
#[iced_cache(Clone, Debug)]
struct SystemEvent {
    timestamp: SystemTime,

    event_id: Option<OwnedEventId>,
    sender: OwnedUserId,
    sender_profile: TimelineDetails<Arc<TimelineProfile>>,

    avatar_cache: AvatarCache,

    content: Arc<SystemMessage>,
}

#[derive(Debug)]
enum SystemMessage {
    MembershipChange(Box<RoomMembershipChange>),
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
