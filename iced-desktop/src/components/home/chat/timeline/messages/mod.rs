use crate::{
    common::*,
    components::{OpenProfileOverlayExt, home::overlay::MediaOverlayParams},
};
use std::{
    collections::BTreeSet,
    fmt::Debug,
    time::{Duration, SystemTime},
};

use chrono_tz::Tz;
use deplace_core::{
    formatting::format_date_divider,
    rich_text::{FormattedBody, Mention},
    settings::{DataSizeUnit, DateFormat, HourFormat, SystemMessageType},
    state::cache::VideoCache,
};
use enumset::EnumSet;
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
    EventSendState, MemberProfileChange, ReactionsByKeyBySender, RoomMembershipChange,
    TimelineDetails, TimelineEventShieldState,
};

mod convert;
mod formatted_body;
mod render;

pub use convert::ToTimelineItem;
use render::render_event;

/// The message an item in the timeline can receive
#[derive(Debug, Clone)]
pub enum TimelineItemMessage {
    None,
    NeedsMedia(NeedsMedia),
    EventEnter,
    EventExit,
    MediaMouseEnter,
    MediaMouseLeave,
    ToggleVideoPause(Arc<iced_video_player::Video>),
    SetIsReplyingTo(OwnedEventId),
    SetIsEditing(bool),
    MessageEventBounds(Rectangle),
    OpenProfileOverlay {
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        bounds: Rectangle,
    },
    HelpHover(Option<HelpKey>),
    OpenPinMenu {
        event_id: OwnedEventId,
        is_pinned: bool,
    },
    OpenDeleteMenu(OwnedEventId),
    Mention(Mention),
    LinkClick(String),
    OpenMediaOverlay(MediaOverlayParams),
}

impl OpenProfileOverlayExt for TimelineItemMessage {
    fn open_profile(room_id: OwnedRoomId, user_id: OwnedUserId, bounds: Rectangle) -> Self {
        Self::OpenProfileOverlay {
            room_id,
            user_id,
            bounds,
        }
    }
}

impl NeedsAvatarExt for TimelineItemMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        TimelineItemMessage::NeedsMedia(NeedsMedia::Avatar { uri })
    }
}

pub enum TimelineItemAction {
    Update,
    NeedsMedia(NeedsMedia),
    SetIsReplyingTo {
        message: Arc<MessageEvent>,
        event_id: OwnedEventId,
    },
    MessageEventBounds(Rectangle),
    HoverChanged(bool),
    OpenProfileOverlay {
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        bounds: Rectangle,
    },
    HelpHover(Option<HelpKey>),
    OpenPinMenu {
        event_id: OwnedEventId,
        is_pinned: bool,
    },
    OpenDeleteMenu(OwnedEventId),
    Mention(Mention),
    LinkClick(String),
    OpenMediaOverlay(MediaOverlayParams),
}

/// An item in the timeline
///
/// This is the most abstract version, able to represent anything
/// in the timeline.
#[iced_cache(Clone)]
pub struct TimelineItem {
    #[hash]
    pub id: String,
    #[hash]
    room_id: OwnedRoomId,
    #[hash]
    kind: TimelineItemKind,
}

impl Debug for TimelineItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.kind.display_string())
    }
}

impl TimelineItem {
    pub fn event_id(&self) -> Option<OwnedEventId> {
        let TimelineItemKind::Message { message: event, .. } = &self.kind else {
            return None;
        };

        event.event_id.clone()
    }

    pub fn event_id_ref(&self) -> Option<&OwnedEventId> {
        let TimelineItemKind::Message { message: event, .. } = &self.kind else {
            return None;
        };

        event.event_id.as_ref()
    }

    pub fn is_pinned(&self) -> bool {
        matches!(&self.kind, TimelineItemKind::Message { message, .. } if message.is_pinned)
    }

    pub fn set_pinned(&mut self, pinned: bool) {
        if let TimelineItemKind::Message { message, .. } = &mut self.kind {
            Arc::make_mut(message).is_pinned = pinned;
        }
    }

    pub fn is_date_divider(&self) -> bool {
        matches!(self.kind, TimelineItemKind::DateDivider { .. })
    }

    pub fn recompute_datedivider_types<'a>(
        &mut self,
        rest: impl Iterator<Item = &'a TimelineItem>,
    ) {
        let TimelineItemKind::DateDivider {
            depends_on_system_messages,
            ..
        } = &mut self.kind
        else {
            return;
        };

        *depends_on_system_messages = None;

        let mut types = EnumSet::empty();
        for item in rest {
            if let TimelineItemKind::DateDivider { .. } = item.kind {
                break;
            }

            let TimelineItemKind::System { event, .. } = &item.kind else {
                return;
            };

            types.insert(event.message_type());
        }

        *depends_on_system_messages = Some(types);
    }

    pub fn remove_replying(&mut self) {
        if let TimelineItemKind::Message { message, .. } = &mut self.kind {
            Arc::make_mut(message).is_replying_to = false;
        }
    }

    pub fn message_event(&self) -> Option<Arc<MessageEvent>> {
        let TimelineItemKind::Message { message: event, .. } = &self.kind else {
            return None;
        };
        Some(event.clone())
    }

    pub fn booleans(&self) -> (bool, bool, bool) {
        let TimelineItemKind::Message {
            is_own,
            is_editable,
            can_be_replied_to,
            ..
        } = &self.kind
        else {
            return (false, false, false);
        };
        (*is_own, *is_editable, *can_be_replied_to)
    }

    pub fn is_hovered(&self) -> bool {
        match &self.kind {
            TimelineItemKind::Message { is_hovered, .. } => *is_hovered,
            TimelineItemKind::System { is_hovered, .. } => *is_hovered,
            _ => false,
        }
    }

    pub fn set_is_hovered(&mut self, hovered: bool) {
        match &mut self.kind {
            TimelineItemKind::Message { is_hovered, .. } => *is_hovered = hovered,
            TimelineItemKind::System { is_hovered, .. } => *is_hovered = hovered,
            _ => {}
        }
    }

    /// Computes what `connects_before` should be for this
    /// item given its neighbors. Pure, so the item can only be mutated in the list if necessary.
    pub fn compute_connection(&self, previous: Option<&TimelineItem>) -> Option<bool> {
        let TimelineItemKind::Message { message: event, .. } = &self.kind else {
            return None;
        };
        let connection_duration = Duration::from_mins(5);

        let connects_before = if !event.in_reply_to.is_empty() {
            false
        } else if let Some(TimelineItemKind::Message {
            message: previous_event,
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
        if let TimelineItemKind::Message { message: event, .. } = &mut self.kind {
            let event = Arc::make_mut(event);
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
        if let TimelineItemKind::Message { message: event, .. } = &mut self.kind {
            let event = Arc::make_mut(event);
            Some(&mut event.avatar_states_for_hash)
        } else {
            None
        }
    }

    fn thumbnail_hashes(&mut self) -> Option<&mut BTreeSet<(String, u64, u64)>> {
        if let TimelineItemKind::Message { message: event, .. } = &mut self.kind {
            let event = Arc::make_mut(event);
            if let MessageContent::Image { image, .. } = &mut event.content {
                Some(&mut image.thumbnail_states_for_hash)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn video_hashes(&mut self) -> Option<&mut BTreeSet<String>> {
        if let TimelineItemKind::Message { message: event, .. } = &mut self.kind {
            let event = Arc::make_mut(event);
            if let MessageContent::Video { video, .. } = &mut event.content {
                Some(&mut video.video_states_for_hash)
            } else {
                None
            }
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

    pub fn update(&mut self, message: TimelineItemMessage) -> Option<TimelineItemAction> {
        match message {
            TimelineItemMessage::None => None,
            TimelineItemMessage::OpenMediaOverlay(params) => {
                Some(TimelineItemAction::OpenMediaOverlay(params))
            }
            TimelineItemMessage::LinkClick(link) => Some(TimelineItemAction::LinkClick(link)),
            TimelineItemMessage::Mention(mention) => Some(TimelineItemAction::Mention(mention)),
            TimelineItemMessage::OpenPinMenu {
                event_id,
                is_pinned,
            } => Some(TimelineItemAction::OpenPinMenu {
                event_id,
                is_pinned,
            }),
            TimelineItemMessage::OpenDeleteMenu(event_id) => {
                Some(TimelineItemAction::OpenDeleteMenu(event_id))
            }
            TimelineItemMessage::HelpHover(help) => Some(TimelineItemAction::HelpHover(help)),
            TimelineItemMessage::OpenProfileOverlay {
                room_id,
                user_id,
                bounds,
            } => Some(TimelineItemAction::OpenProfileOverlay {
                room_id,
                user_id,
                bounds,
            }),
            TimelineItemMessage::MessageEventBounds(bounds) => {
                Some(TimelineItemAction::MessageEventBounds(bounds))
            }
            TimelineItemMessage::SetIsEditing(_) => None,
            TimelineItemMessage::SetIsReplyingTo(event_id) => {
                if let TimelineItemKind::Message { message, .. } = &mut self.kind {
                    Arc::make_mut(message).is_replying_to = true;
                    Some(TimelineItemAction::SetIsReplyingTo {
                        message: message.clone(),
                        event_id,
                    })
                } else {
                    None
                }
            }
            TimelineItemMessage::MediaMouseEnter => {
                if let TimelineItemKind::Message { message: event, .. } = &mut self.kind {
                    let event = Arc::make_mut(event);
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
                if let TimelineItemKind::Message { message: event, .. } = &mut self.kind {
                    let event = Arc::make_mut(event);
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
            TimelineItemMessage::EventEnter => {
                self.set_hovered(true);
                Some(TimelineItemAction::HoverChanged(true))
            }
            TimelineItemMessage::EventExit => {
                self.set_hovered(false);
                Some(TimelineItemAction::HoverChanged(false))
            }
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

    pub fn view(
        &self,
        theme: Theme,
        structure: Structure,
        is_focused: bool,
        help_state: HelpState<HelpKey>,
        index: usize,
    ) -> iced::Element<'static, TimelineItemMessage> {
        let fallback = || -> iced::Element<'static, TimelineItemMessage> {
            w::text(format!("{:?}", self))
                .color(theme.colors.error)
                .size(structure.chat.text_size)
                .into()
        };
        let render_error_message = |text: String| {
            w::text(text)
                .color(theme.colors.error)
                .size(structure.chat.text_size)
                .into()
        };

        let room_id = self.room_id.clone();

        match &self.kind {
            TimelineItemKind::DateDivider {
                date,
                depends_on_system_messages,
                system_messages_to_show,
            } => {
                if depends_on_system_messages
                    .is_some_and(|dep| system_messages_to_show.borrow().is_disjoint(dep))
                {
                    return w::space().into();
                }
                w::row![
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
                .into()
            }
            TimelineItemKind::FailedToParseMessageLike { event_type, error } => {
                render_error_message(format!(
                    "Failed to parse message like: event_type={event_type:?}, error={error:?}"
                ))
            }
            TimelineItemKind::FailedToParseState {
                event_type,
                state_key,
                error,
            } => render_error_message(format!(
                "Failed to parse state: event_type={event_type:?}, state_key={state_key:?}, error={error:?}"
            )),
            TimelineItemKind::ReadMarker => w::container(
                w::container(Space::new())
                    .width(Fill)
                    .height(structure.divider_width)
                    .style(move |_| ContainerStyle::default().background(theme.accent)),
            )
            .padding(padding::vertical(structure.small_gap))
            .into(),
            TimelineItemKind::TimelineStart => fallback(),
            TimelineItemKind::System {
                is_hovered,
                event,
                previous_is_event,
                system_messages_to_show,
            } => {
                if !event.should_show(*system_messages_to_show.borrow()) {
                    return w::space().into();
                }

                let is_hovered = *is_hovered;

                render_event(
                    w::lazy(event.clone(), move |event| {
                        event.view(theme, room_id.clone(), structure, is_hovered)
                    }),
                    structure,
                    theme,
                    is_hovered,
                    false,
                    *previous_is_event,
                    false,
                    structure.small_gap,
                )
            }
            TimelineItemKind::Message {
                message: event,
                is_hovered,
                previous_is_event,
                ..
            } => {
                let is_hovered = *is_hovered;

                let previous_is_event = *previous_is_event;
                let connects_previous = event.connects_previous;

                render_event(
                    w::lazy(
                        (event.clone(), help_state, index),
                        move |(event, help_state, index)| {
                            event.view(
                                room_id.clone(),
                                theme,
                                structure,
                                false,
                                *help_state,
                                *index,
                                is_hovered,
                            )
                        },
                    ),
                    structure,
                    theme,
                    is_hovered,
                    is_focused,
                    previous_is_event,
                    connects_previous,
                    structure.gap * 1.5,
                )
            }
        }
    }
}

#[derive(Clone)]
enum TimelineItemKind {
    DateDivider {
        date: SystemTime,
        depends_on_system_messages: Option<EnumSet<SystemMessageType>>,
        system_messages_to_show: Receiver<EnumSet<SystemMessageType>>,
    },
    TimelineStart,
    ReadMarker,
    Message {
        message: Arc<MessageEvent>,
        is_editable: bool,
        can_be_replied_to: bool,
        is_hovered: bool,
        is_own: bool,
        previous_is_event: bool,
    },
    System {
        is_hovered: bool,
        event: Box<SystemEvent>,
        previous_is_event: bool,
        system_messages_to_show: Receiver<EnumSet<SystemMessageType>>,
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

impl DisplayString for TimelineItemKind {
    fn display_string(&self) -> String {
        match self {
            TimelineItemKind::DateDivider { .. } => "DateDivider".to_string(),
            TimelineItemKind::FailedToParseMessageLike { .. } => {
                "FailedToParseMessageLike".to_string()
            }
            TimelineItemKind::FailedToParseState { .. } => "FailedToParseState".to_string(),
            TimelineItemKind::TimelineStart => "TimelineStart".to_string(),
            TimelineItemKind::ReadMarker => "ReadMarker".to_string(),
            TimelineItemKind::Message { .. } => "Message".to_string(),
            TimelineItemKind::System { .. } => "System".to_string(),
        }
    }
}

impl Hash for TimelineItemKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            TimelineItemKind::Message {
                message: event,
                is_hovered,
                previous_is_event: previous_is_text_message,
                ..
            } => {
                event.hash(state);
                is_hovered.hash(state);
                previous_is_text_message.hash(state);
            }
            TimelineItemKind::System {
                is_hovered,
                previous_is_event,
                system_messages_to_show,
                ..
            } => {
                is_hovered.hash(state);
                previous_is_event.hash(state);
                system_messages_to_show.borrow().hash(state);
            }
            TimelineItemKind::DateDivider {
                depends_on_system_messages,
                system_messages_to_show,
                ..
            } => {
                depends_on_system_messages.hash(state);
                system_messages_to_show.borrow().hash(state);
            }
            // Other things are static, so hashing doesn't need to include them
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimelineProfile {
    pub display_name: Arc<Option<String>>,
    pub avatar_url: Arc<Option<OwnedMxcUri>>,
    user_id: Arc<OwnedUserId>,
}

impl ProfileLike for TimelineProfile {
    type Id<'a>
        = &'a UserId
    where
        Self: 'a;

    fn icon_border_radius_ratio(&self) -> f32 {
        0.5
    }

    fn profile_name(&self) -> Option<String> {
        (*self.display_name).clone()
    }

    fn get_avatar(&self) -> Option<OwnedMxcUri> {
        (*self.avatar_url).clone()
    }

    fn profile_id(&self) -> Self::Id<'_> {
        &self.user_id
    }
}

#[iced_cache(Debug, Clone)]
struct MessageEvent {
    state: AppState,
    membership_map: Receiver<MembershipMap>,

    timestamp: SystemTime,

    event_id: Option<OwnedEventId>,
    sender: OwnedUserId,
    sender_profile: TimelineDetails<TimelineProfile>,

    in_reply_to: Arc<Vec<ReplyToDetails>>,

    reactions: Arc<ReactionsByKeyBySender>,

    timezone: Receiver<Tz>,
    hour_format: Receiver<HourFormat>,
    date_format: Receiver<DateFormat>,

    /// Weather the previous message is the same type, sender and withing 5 minutes of this message.
    connects_previous: bool,

    #[hash]
    is_replying_to: bool,

    avatar_cache: AvatarCache,

    is_highlighted: bool,
    contains_only_emojis: bool,

    #[hash]
    is_pinned: bool,
    #[hash]
    is_edited: bool,

    shield: TimelineEventShieldState,

    #[hash]
    content: MessageContent,

    send_state: Option<EventSendState>,
}

impl ExtraHash for MessageEvent {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.date_format.borrow().hash(state);
        self.hour_format.borrow().hash(state);
        self.timezone.borrow().hash(state);

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
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub size: Option<u64>,
    pub thumbnail_source: Option<MediaSource>,
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
        data_size_unit: Receiver<DataSizeUnit>,
    },
    Image {
        is_hovered: bool,
        image: ImageMessage,
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
        is_hovered: bool,
        video: VideoMessage,
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
                formatted_body.hash(state);
            }
            MessageContent::File {
                caption,
                formatted_caption,
                filename,
                source,
                info,
                data_size_unit,
            } => {
                caption.hash(state);
                formatted_caption.hash(state);
                filename.hash(state);
                source.unique_key().hash(state);
                data_size_unit.borrow().hash(state);
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
    formatted_caption: Option<FormattedBody>,

    pub blur_preview: Option<ImageHandle>,

    thumbnail_cache: ThumbnailCache,

    #[hash]
    pub filename: String,
    pub source: MediaSource,
    #[hash]
    pub info: Option<VisualInfo>,

    pub data_size_unit: Receiver<DataSizeUnit>,
}

impl ExtraHash for ImageMessage {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.unique_key().hash(state);
        self.data_size_unit.borrow().hash(state);
    }
}

#[iced_cache(Debug, Clone)]
pub struct VideoMessage {
    #[hash]
    caption: Option<String>,
    #[hash]
    formatted_caption: Option<FormattedBody>,

    blur_preview: Option<ImageHandle>,

    video_cache: VideoCache,

    #[hash]
    filename: String,
    source: MediaSource,
    #[hash]
    info: Option<VisualInfo>,

    data_size_unit: Receiver<DataSizeUnit>,
}

impl ExtraHash for VideoMessage {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source.unique_key().hash(state);
        self.data_size_unit.borrow().hash(state);
    }
}

#[derive(Debug)]
struct ReplyToDetails {
    event_id: OwnedEventId,
    event: TimelineDetails<ReplyEvent>,
}

#[derive(Debug)]
struct ReplyEvent {
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
    sender_profile: TimelineDetails<TimelineProfile>,

    timezone: Receiver<Tz>,
    hour_format: Receiver<HourFormat>,
    date_format: Receiver<DateFormat>,

    avatar_cache: AvatarCache,

    content: Arc<SystemMessage>,
}

impl ExtraHash for SystemEvent {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.timezone.borrow().hash(state);
        self.hour_format.borrow().hash(state);
        self.date_format.borrow().hash(state);
    }
}

impl SystemEvent {
    pub fn message_type(&self) -> SystemMessageType {
        match *self.content {
            SystemMessage::CallInvite => SystemMessageType::CallInvite,
            SystemMessage::CallMember => SystemMessageType::CallMember,
            SystemMessage::MembershipChange(_) => SystemMessageType::MembershipChange,
            SystemMessage::PolicyRuleRoom => SystemMessageType::PolicyRuleRoom,
            SystemMessage::PolicyRuleServer => SystemMessageType::PolicyRuleServer,
            SystemMessage::PolicyRuleUser => SystemMessageType::PolicyRuleUser,
            SystemMessage::RoomAvatar(_) => SystemMessageType::RoomAvatar,
            SystemMessage::RoomCanonicalAlias(_) => SystemMessageType::RoomCanonicalAlias,
            SystemMessage::RoomCreate => SystemMessageType::RoomCreate,
            SystemMessage::RoomEncryption => SystemMessageType::RoomEncryption,
            SystemMessage::RoomGuestAccess(_) => SystemMessageType::RoomGuestAccess,
            SystemMessage::RoomHistoryVisibility(_) => SystemMessageType::RoomHistoryVisibility,
            SystemMessage::RoomJoinRules(_) => SystemMessageType::RoomJoinRules,
            SystemMessage::RoomName(_) => SystemMessageType::RoomName,
            SystemMessage::RoomPinnedEvents => SystemMessageType::RoomPinnedEvents,
            SystemMessage::RoomPowerLevels => SystemMessageType::RoomPowerLevels,
            SystemMessage::RoomServerAcl => SystemMessageType::RoomServerAcl,
            SystemMessage::RoomThirdPartyInvite => SystemMessageType::RoomThirdPartyInvite,
            SystemMessage::RoomTombstone => SystemMessageType::RoomTombstone,
            SystemMessage::RoomTopic(_) => SystemMessageType::RoomTopic,
            SystemMessage::SpaceChild => SystemMessageType::SpaceChild,
            SystemMessage::SpaceParent => SystemMessageType::SpaceParent,
            SystemMessage::ProfileChange(_) => SystemMessageType::ProfileChange,
            SystemMessage::RtcNotification(RtcNotification { .. }) => {
                SystemMessageType::RtcNotification
            }
            SystemMessage::Custom { .. } => SystemMessageType::Custom,
        }
    }

    pub fn should_show(&self, allowed: EnumSet<SystemMessageType>) -> bool {
        allowed.contains(self.message_type())
    }
}

#[derive(Debug)]
struct RtcNotification {
    call_intent: Option<CallIntent>,
    declined_by: Vec<OwnedUserId>,
    call_started: Option<SystemTime>,
    current_members: Option<BTreeSet<OwnedUserId>>,
}

#[derive(Debug)]
enum SystemMessage {
    MembershipChange(Box<RoomMembershipChange>),
    ProfileChange(Box<MemberProfileChange>),
    CallInvite,
    CallMember,
    RtcNotification(RtcNotification),
    Custom { event_type: String },

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
