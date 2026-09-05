use crate::common::*;
use std::{collections::BTreeSet, time::SystemTime};

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
    kind: TimelineItemKind,
}

impl TimelineItem {
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

    pub fn load_media(&mut self, media: &MediaLoaded) -> bool {
        let id = self.id.clone();
        let result = match media {
            MediaLoaded::Avatar { uri } if let Some(hashes) = self.avatar_hashes() => {
                hashes.remove(uri)
            }
            MediaLoaded::Thumbnail { key } if let Some(hashes) = self.thumbnail_hashes() => {
                hashes.remove(key)
            }
            MediaLoaded::Video { key } if let Some(hashes) = self.video_hashes() => {
                hashes.remove(key)
            }
            _ => false,
        };
        tracing::debug!("[diag] TimelineItem({id}).load_media({media:?}) -> {result}");
        result
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
                w::mouse_area(
                    w::container(w::lazy(event.clone(), move |event| {
                        event.view(theme, structure)
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
                w::mouse_area(
                    w::container(w::lazy(event.clone(), move |event| {
                        event.view(theme, structure)
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
        event: Box<SystemEvent>,
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

    // Arc-wrapped because `MessageEvent` gets deep-cloned on every render
    // pass (`w::lazy` must clone its dependency before it can even check
    // the hash to decide whether to skip re-rendering) -- without this,
    // a message with many reactions makes every touch of its row
    // expensive regardless of how many other messages are in the room.
    reactions: Arc<ReactionsByKeyBySender>,

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
