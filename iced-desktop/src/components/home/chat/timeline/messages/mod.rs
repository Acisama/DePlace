use crate::common::*;
use std::time::SystemTime;

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

/// The message an item in the timeline can receive
#[derive(Debug, Clone)]
pub enum TimelineItemMessage {
    NeedsMedia(NeedsMedia),
    MediaLoaded(MediaLoaded),
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
    kind: TimelineItemKind,

    avatar_cache: AvatarCache,
    thumbnail_cache: ThumbnailCache,
    video_cache: VideoCache,
}

impl TimelineItem {
    pub fn load_media(&mut self, media: &MediaLoaded) {
        match media {
            MediaLoaded::Avatar { uri } => {
                self.avatar_states_for_hash.remove(uri);
            }
            MediaLoaded::Thumbnail { key } => {
                self.thumbnail_states_for_hash.remove(key);
            }
            MediaLoaded::Video { key } => {
                self.video_states_for_hash.remove(key);
            }
        }
    }

    fn set_hovered(&mut self, new_hovered: bool) -> Option<TimelineItemAction> {
        match &mut self.kind {
            TimelineItemKind::Message { is_hovered, .. } => {
                if *is_hovered != new_hovered {
                    *is_hovered = new_hovered;
                    return Some(TimelineItemAction::Update);
                }
            }
            TimelineItemKind::System { is_hovered, .. } => {
                if *is_hovered != new_hovered {
                    *is_hovered = new_hovered;
                    return Some(TimelineItemAction::Update);
                }
            }
            _ => {}
        }
        None
    }
}

impl IcedWidget<TimelineItemMessage, TimelineItemAction> for TimelineItem {
    fn update(&mut self, message: TimelineItemMessage) -> Option<TimelineItemAction> {
        match message {
            TimelineItemMessage::MediaMouseEnter
                if let TimelineItemKind::Message { event, .. } = &mut self.kind =>
            {
                event.media_hovered = true;
                Some(TimelineItemAction::Update)
            }
            TimelineItemMessage::MediaMouseLeave
                if let TimelineItemKind::Message { event, .. } = &mut self.kind =>
            {
                event.media_hovered = false;
                Some(TimelineItemAction::Update)
            }
            TimelineItemMessage::EventEnter => self.set_hovered(true),
            TimelineItemMessage::EventExit => self.set_hovered(false),
            TimelineItemMessage::NeedsMedia(media) => {
                match &media {
                    NeedsMedia::Avatar { uri } => self.avatar_states_for_hash.insert(uri.clone()),
                    NeedsMedia::Thumbnail { key, .. } => {
                        self.thumbnail_states_for_hash.insert(key.clone())
                    }
                    NeedsMedia::Video { source, .. } => {
                        self.video_states_for_hash.insert(source.unique_key())
                    }
                };
                Some(TimelineItemAction::NeedsMedia(media))
            }
            TimelineItemMessage::MediaLoaded(media) => {
                match media {
                    MediaLoaded::Avatar { uri } => self.avatar_states_for_hash.remove(&uri),
                    MediaLoaded::Thumbnail { key } => self.thumbnail_states_for_hash.remove(&key),
                    MediaLoaded::Video { key } => self.video_states_for_hash.remove(&key),
                };
                None
            }
            TimelineItemMessage::ToggleVideoPause(video) => {
                video.set_paused(!video.paused());
                None
            }
            TimelineItemMessage::MediaMouseEnter | TimelineItemMessage::MediaMouseLeave => None,
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
            TimelineItemKind::System { is_hovered, event } => {
                let is_hovered = *is_hovered;
                let avatar_cache = self.avatar_cache.clone();
                let event = event.clone();
                w::mouse_area(
                    // System events are static and do not need to be updated
                    w::container(w::lazy((), move |_| {
                        event.view(theme, structure, avatar_cache.clone())
                    }))
                    .width(Fill)
                    .style(move |_| {
                        ContainerStyle::default().border(
                            border::rounded(structure.inner_border_radius)
                                .width(structure.border_thickness)
                                .color(if is_hovered {
                                    theme.border
                                } else {
                                    Color::TRANSPARENT
                                }),
                        )
                    })
                    .padding(structure.small_gap / 2.0),
                )
                .on_enter(TimelineItemMessage::EventEnter)
                .on_exit(TimelineItemMessage::EventExit)
                .into()
            }
            TimelineItemKind::Message { event, is_hovered } => {
                let is_hovered = *is_hovered;
                let avatar_cache = self.avatar_cache.clone();
                let thumbnail_cache = self.thumbnail_cache.clone();
                let video_cache = self.video_cache.clone();
                w::mouse_area(
                    w::container(w::lazy(event.clone(), move |event| {
                        let avatar_cache = avatar_cache.clone();
                        let thumbnail_cache = thumbnail_cache.clone();
                        let video_cache = video_cache.clone();
                        event.view(
                            theme,
                            structure,
                            avatar_cache.clone(),
                            thumbnail_cache.clone(),
                            video_cache.clone(),
                        )
                    }))
                    .style(move |_| ContainerStyle {
                        background: None,
                        border: Border {
                            color: if is_hovered {
                                theme.border
                            } else {
                                Color::TRANSPARENT
                            },
                            width: structure.border_thickness,
                            radius: structure.semi_border_radius().into(),
                        },
                        ..Default::default()
                    })
                    .width(Fill),
                )
                .on_enter(TimelineItemMessage::EventEnter)
                .on_exit(TimelineItemMessage::EventExit)
                .into()
            }
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
    },
    System {
        is_hovered: bool,
        event: Arc<SystemEvent>,
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
            TimelineItemKind::Message { event, is_hovered } => {
                event.hash(state);
                is_hovered.hash(state);
            }
            TimelineItemKind::System { is_hovered, .. } => {
                is_hovered.hash(state);
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

/// This has an empty hash implementation because it doesn't change
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
