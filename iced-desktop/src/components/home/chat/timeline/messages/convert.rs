use matrix_sdk::ruma::events::StateEventType;
use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::common::*;
use crate::components::home::chat::timeline::messages::{RtcNotification, TimelineProfile};
use crate::components::{blurhash_to_image, thumbhash_to_image};

use super::{
    ImageMessage, MessageContent, MessageEvent, ReplyContent, ReplyEvent, ReplyToDetails,
    SystemEvent, SystemMessage, VideoMessage, VisualInfo,
};
use super::{TimelineItem, TimelineItemKind};

use matrix_sdk::ruma::events::room::ImageInfo;
use matrix_sdk::ruma::events::room::message::{MessageType, VideoInfo};
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange, EmbeddedEvent, TimelineItemKind as UiTimelineItemKind,
};
use matrix_sdk_ui::timeline::{InReplyToDetails, VirtualTimelineItem};
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent};
use matrix_sdk_ui::timeline::{TimelineDetails, TimelineItem as UiTimelineItem};

pub trait ToTimelineItem {
    fn convert(self, state: &AppState, room_id: OwnedRoomId) -> TimelineItem;
}

impl ToTimelineItem for Arc<UiTimelineItem> {
    fn convert(self, state: &AppState, room_id: OwnedRoomId) -> TimelineItem {
        TimelineItem {
            id: self.unique_id().0.clone(),
            room_id,
            kind: TimelineItemKind::from_ui(self.kind(), state),
        }
    }
}

impl TimelineItemKind {
    fn from_ui(value: &UiTimelineItemKind, state: &AppState) -> Self {
        match value {
            UiTimelineItemKind::Virtual(virt) => match virt {
                VirtualTimelineItem::ReadMarker => TimelineItemKind::ReadMarker,
                VirtualTimelineItem::TimelineStart => TimelineItemKind::TimelineStart,
                VirtualTimelineItem::DateDivider(ms) => TimelineItemKind::DateDivider {
                    date: ms.to_system_time().unwrap_or_else(SystemTime::now),
                    depends_on_system_messages: None,
                    system_messages_to_show: state.settings().system_messages_to_show.watch(),
                },
            },
            UiTimelineItemKind::Event(event) => {
                let timestamp = event
                    .timestamp()
                    .to_system_time()
                    .unwrap_or_else(SystemTime::now);
                let sender = event.sender().to_owned();
                let sender_profile = match event.sender_profile().clone() {
                    TimelineDetails::Pending => TimelineDetails::Pending,
                    TimelineDetails::Error(e) => TimelineDetails::Error(e),
                    TimelineDetails::Unavailable => TimelineDetails::Unavailable,
                    TimelineDetails::Ready(p) => {
                        TimelineDetails::Ready(Arc::new(TimelineProfile {
                            display_name: p.display_name,
                            avatar_url: p.avatar_url,
                            user_id: sender.clone(),
                        }))
                    }
                };
                let event_id = event.event_id().map(|id| id.to_owned());

                // With a normal closure, there would be move problems
                macro_rules! system {
                    ($content:expr) => {
                        TimelineItemKind::System {
                            is_hovered: false,
                            previous_is_event: false,
                            system_messages_to_show: state
                                .settings()
                                .system_messages_to_show
                                .watch(),
                            event: Box::new(SystemEvent {
                                timestamp,
                                event_id,
                                sender,
                                sender_profile,
                                content: Arc::new($content),

                                avatar_cache: state.avatar_cache().clone(),
                                avatar_states_for_hash: BTreeSet::new(),
                            }),
                        }
                    };
                }

                match event.content() {
                    TimelineItemContent::CallInvite => system!(SystemMessage::CallInvite),
                    TimelineItemContent::FailedToParseMessageLike { event_type, error } => {
                        TimelineItemKind::FailedToParseMessageLike {
                            event_type: Arc::new(event_type.to_string()),
                            error: error.clone(),
                        }
                    }
                    TimelineItemContent::FailedToParseState {
                        event_type,
                        state_key,
                        error,
                    } => TimelineItemKind::FailedToParseState {
                        event_type: Arc::new(event_type.to_string()),
                        state_key: Arc::new(state_key.to_string()),
                        error: error.clone(),
                    },
                    TimelineItemContent::MembershipChange(c) => {
                        system!(SystemMessage::MembershipChange(Box::new(c.clone())))
                    }
                    TimelineItemContent::OtherState(other) => match other.content() {
                        AnyOtherStateEventContentChange::PolicyRuleRoom(_) => {
                            system!(SystemMessage::PolicyRuleRoom)
                        }
                        AnyOtherStateEventContentChange::PolicyRuleServer(_) => {
                            system!(SystemMessage::PolicyRuleServer)
                        }
                        AnyOtherStateEventContentChange::PolicyRuleUser(_) => {
                            system!(SystemMessage::PolicyRuleUser)
                        }
                        AnyOtherStateEventContentChange::RoomAvatar(change) => {
                            let change = get_change!(change, |c| c.url.clone());
                            system!(SystemMessage::RoomAvatar(change))
                        }
                        AnyOtherStateEventContentChange::RoomCanonicalAlias(change) => {
                            let change = get_change!(change, |c| c.alias.clone());
                            system!(SystemMessage::RoomCanonicalAlias(change))
                        }
                        AnyOtherStateEventContentChange::RoomCreate(_) => {
                            system!(SystemMessage::RoomCreate)
                        }
                        AnyOtherStateEventContentChange::RoomEncryption(_) => {
                            system!(SystemMessage::RoomEncryption)
                        }
                        AnyOtherStateEventContentChange::RoomGuestAccess(change) => {
                            let change = get_current_and_prev(
                                change,
                                |c| Some(c.guest_access.clone()),
                                |c| c.guest_access.clone(),
                            );
                            system!(SystemMessage::RoomGuestAccess(change))
                        }
                        AnyOtherStateEventContentChange::RoomHistoryVisibility(change) => {
                            let change =
                                get_change!(change, |c| Some(c.history_visibility.clone()));
                            system!(SystemMessage::RoomHistoryVisibility(change))
                        }
                        AnyOtherStateEventContentChange::RoomJoinRules(change) => {
                            let change = get_change!(change, |c| Some(c.join_rule.clone()));
                            system!(SystemMessage::RoomJoinRules(change))
                        }
                        AnyOtherStateEventContentChange::RoomName(change) => {
                            let change = get_current_and_prev(
                                change,
                                |c| Some(c.name.clone()),
                                |c| c.name.clone(),
                            );
                            system!(SystemMessage::RoomName(change))
                        }
                        AnyOtherStateEventContentChange::RoomPinnedEvents(_) => {
                            system!(SystemMessage::RoomPinnedEvents)
                        }
                        AnyOtherStateEventContentChange::RoomPowerLevels(_) => {
                            system!(SystemMessage::RoomPowerLevels)
                        }
                        AnyOtherStateEventContentChange::RoomServerAcl(_) => {
                            system!(SystemMessage::RoomServerAcl)
                        }
                        AnyOtherStateEventContentChange::RoomThirdPartyInvite(_) => {
                            system!(SystemMessage::RoomThirdPartyInvite)
                        }
                        AnyOtherStateEventContentChange::RoomTombstone(_) => {
                            system!(SystemMessage::RoomTombstone)
                        }
                        AnyOtherStateEventContentChange::RoomTopic(change) => {
                            let change = get_current_and_prev(
                                change,
                                |c| Some(c.topic.clone()),
                                |c| c.topic.clone(),
                            );
                            system!(SystemMessage::RoomTopic(change))
                        }
                        AnyOtherStateEventContentChange::SpaceChild(_) => {
                            system!(SystemMessage::SpaceChild)
                        }
                        AnyOtherStateEventContentChange::SpaceParent(_) => {
                            system!(SystemMessage::SpaceParent)
                        }
                        other => match other.event_type() {
                            StateEventType::CallMember => {
                                system!(SystemMessage::CallMember)
                            }
                            _ => {
                                system!(SystemMessage::Custom {
                                    event_type: other.event_type().to_string()
                                })
                            }
                        },
                    },
                    TimelineItemContent::ProfileChange(p) => {
                        system!(SystemMessage::ProfileChange(Box::new(p.clone())))
                    }
                    TimelineItemContent::RtcNotification {
                        call_intent,
                        declined_by,
                        active_call_info,
                    } => system!(SystemMessage::RtcNotification(RtcNotification {
                        call_intent: call_intent.clone(),
                        declined_by: declined_by.clone(),
                        call_started: active_call_info
                            .as_ref()
                            .and_then(|info| info.call_started_ts_millis)
                            .and_then(|t| t.to_system_time()),
                        currnet_members: active_call_info.as_ref().map(|info| info
                            .active_members
                            .iter()
                            .cloned()
                            .collect()),
                    })),
                    TimelineItemContent::MsgLike(m) => {
                        let settings = state.settings();
                        TimelineItemKind::Message {
                            is_own: event.is_own(),
                            is_editable: event.is_editable(),
                            can_be_replied_to: event.can_be_replied_to(),
                            message: Arc::new(MessageEvent {
                                timestamp,

                                timezone: settings.timezone.watch(),
                                hour_format: settings.hour_format.watch(),
                                date_format: settings.date_format.watch(),

                                event_id,
                                sender,
                                sender_profile,

                                is_replying_to: false,

                                in_reply_to: Arc::new(
                                    m.in_reply_to
                                        .as_ref()
                                        .map(get_reply_details)
                                        .unwrap_or_default(),
                                ),

                                reactions: Arc::new(m.reactions.clone()),

                                connects_previous: false,

                                is_highlighted: event.is_highlighted(),
                                contains_only_emojis: event.contains_only_emojis(),

                                shield: event.get_shield(false),
                                send_state: event.send_state().cloned(),

                                content: MessageContent::from_ui(&m.kind, state),

                                avatar_cache: state.avatar_cache().clone(),
                                avatar_states_for_hash: BTreeSet::new(),
                            }),
                            is_hovered: false,
                            previous_is_event: false,
                        }
                    }
                }
            }
        }
    }
}

fn string_to_option(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

impl MessageContent {
    fn from_ui(value: &MsgLikeKind, state: &AppState) -> Self {
        let settings = state.settings();

        let thumbnail_cache = state.thumbnail_cache().clone();
        let video_cache = state.video_cache().clone();

        match value {
            MsgLikeKind::Message(msg) => match msg.msgtype() {
                MessageType::Audio(_) => MessageContent::Audio,
                MessageType::Emote(emote) => MessageContent::Emote {
                    body: string_to_option(&emote.body),
                    formatted_body: None,
                },
                MessageType::File(file) => MessageContent::File {
                    caption: file.caption().map(|s| s.to_string()),
                    formatted_caption: None,
                    filename: file.filename().to_string(),
                    source: file.source.clone(),
                    info: file.info.clone(),
                    data_size_unit: settings.data_size_unit.watch(),
                },
                MessageType::Image(image) => MessageContent::Image {
                    image: ImageMessage {
                        caption: image.caption().map(|s| s.to_string()),
                        formatted_caption: None,

                        blur_preview: image.info.as_ref().and_then(|info| {
                            info.thumbhash
                                .as_ref()
                                .map(thumbhash_to_image)
                                .unwrap_or_else(|| {
                                    info.blurhash.as_ref().and_then(|b| blurhash_to_image(b))
                                })
                        }),

                        filename: image.filename().to_string(),
                        source: image.source.clone(),
                        info: image.info.as_ref().map(|info| info.into()),

                        thumbnail_cache: thumbnail_cache.clone(),
                        thumbnail_states_for_hash: BTreeSet::new(),

                        data_size_unit: settings.data_size_unit.watch(),
                    },
                    is_hovered: false,
                },
                MessageType::Location(_) => MessageContent::Location,
                MessageType::Notice(notice) => MessageContent::Notice {
                    body: string_to_option(&notice.body),
                    formatted_body: None,
                },
                MessageType::ServerNotice(server_notice) => MessageContent::ServerNotice {
                    body: string_to_option(&server_notice.body),
                },
                MessageType::Text(text) => MessageContent::Text {
                    body: string_to_option(&text.body),
                    formatted_body: None,
                    _url_previews: text.url_previews.clone(),
                },
                MessageType::VerificationRequest(request) => MessageContent::VerificationRequest {
                    body: string_to_option(&request.body),
                    formatted_body: None,
                },
                MessageType::Video(video) => MessageContent::Video {
                    video: VideoMessage {
                        caption: video.caption().map(|s| s.to_string()),
                        formatted_caption: None,

                        blur_preview: video.info.as_ref().and_then(|info| {
                            info.thumbhash
                                .as_ref()
                                .map(thumbhash_to_image)
                                .unwrap_or_else(|| {
                                    info.blurhash.as_ref().and_then(|b| blurhash_to_image(b))
                                })
                        }),

                        filename: video.filename().to_string(),
                        source: video.source.clone(),
                        info: video.info.as_ref().map(|info| info.into()),

                        video_cache: video_cache.clone(),
                        video_states_for_hash: BTreeSet::new(),

                        data_size_unit: settings.data_size_unit.watch(),
                    },
                    is_hovered: false,
                },
                other => MessageContent::Other {
                    event_type: Arc::new(other.msgtype().to_string()),
                },
            },
            MsgLikeKind::LiveLocation(_) => MessageContent::LiveLocation,
            MsgLikeKind::Other(other) => MessageContent::Other {
                event_type: Arc::new(other.event_type().to_string()),
            },
            MsgLikeKind::Poll(_) => MessageContent::Poll,
            MsgLikeKind::Sticker(_) => MessageContent::Sticker,
            MsgLikeKind::UnableToDecrypt(_) => MessageContent::UnableToDecrypt,
            MsgLikeKind::Redacted => MessageContent::Redacted,
        }
    }
}

impl From<&Box<ImageInfo>> for VisualInfo {
    fn from(value: &Box<ImageInfo>) -> Self {
        Self {
            width: value.width.map(|u| u.into()),
            height: value.height.map(|u| u.into()),
            size: value.size.map(|u| u.into()),
            thumbnail_source: value.thumbnail_source.clone(),
        }
    }
}

impl From<&Box<VideoInfo>> for VisualInfo {
    fn from(value: &Box<VideoInfo>) -> Self {
        Self {
            width: value.width.map(|u| u.into()),
            height: value.height.map(|u| u.into()),
            size: value.size.map(|u| u.into()),
            thumbnail_source: value.thumbnail_source.clone(),
        }
    }
}

fn get_reply_details(event: &InReplyToDetails) -> Vec<ReplyToDetails> {
    let mut replies = Vec::new();

    let mut current_event = event.event.clone();
    let mut current_id = event.event_id.clone();

    loop {
        let ConvertTimelineReplyResult(next_opt, converted) =
            convert_timeline_reply(&current_event, current_id);

        replies.push(converted);

        if let Some((next_event, next_id)) = next_opt {
            current_event = next_event;
            current_id = next_id;
        } else {
            break;
        }
    }

    replies
}

struct ConvertTimelineReplyResult(
    Option<(TimelineDetails<Box<EmbeddedEvent>>, OwnedEventId)>,
    ReplyToDetails,
);

fn convert_timeline_reply(
    event: &TimelineDetails<Box<EmbeddedEvent>>,
    event_id: OwnedEventId,
) -> ConvertTimelineReplyResult {
    let embedded = match event {
        TimelineDetails::Error(e) => {
            return ConvertTimelineReplyResult(
                None,
                ReplyToDetails {
                    event_id,
                    event: TimelineDetails::Error(e.clone()),
                },
            );
        }
        TimelineDetails::Pending => {
            return ConvertTimelineReplyResult(
                None,
                ReplyToDetails {
                    event_id,
                    event: TimelineDetails::Pending,
                },
            );
        }
        TimelineDetails::Unavailable => {
            return ConvertTimelineReplyResult(
                None,
                ReplyToDetails {
                    event_id,
                    event: TimelineDetails::Unavailable,
                },
            );
        }
        TimelineDetails::Ready(emb) => emb,
    };

    let mut next_data = None;

    let content = match &embedded.content {
        TimelineItemContent::CallInvite => ReplyContent::CallInvite,
        TimelineItemContent::FailedToParseMessageLike { .. } => {
            ReplyContent::Error("Failed to parse message".into())
        }
        TimelineItemContent::FailedToParseState { .. } => {
            ReplyContent::Error("Failed to parse state".into())
        }
        TimelineItemContent::MembershipChange(_) => {
            ReplyContent::System("Membership change".into())
        }
        TimelineItemContent::MsgLike(msglike) => {
            if let Some(details) = &msglike.in_reply_to {
                next_data = Some((details.event.clone(), details.event_id.clone()))
            }
            match &msglike.kind {
                MsgLikeKind::LiveLocation(_) => ReplyContent::Location,
                MsgLikeKind::Poll(_) => ReplyContent::Poll,
                MsgLikeKind::Message(msg) => match msg.msgtype() {
                    MessageType::Audio(_) => ReplyContent::Audio,
                    MessageType::Emote(content) => {
                        ReplyContent::Emote(content.body.as_str().into())
                    }
                    MessageType::File(_) | MessageType::Video(_) | MessageType::Image(_) => {
                        ReplyContent::Media
                    }
                    MessageType::Location(_) => ReplyContent::Location,
                    MessageType::Notice(content) => {
                        ReplyContent::Text(content.body.as_str().into())
                    }
                    MessageType::ServerNotice(content) => {
                        ReplyContent::Text(content.body.as_str().into())
                    }
                    MessageType::Text(content) => ReplyContent::Text(content.body.as_str().into()),
                    MessageType::VerificationRequest(_) => {
                        ReplyContent::Text("Verification request".into())
                    }
                    _ => ReplyContent::Error("Unknown message type".into()),
                },
                MsgLikeKind::Redacted => ReplyContent::Redacted,
                MsgLikeKind::Sticker(_) => ReplyContent::Media,
                MsgLikeKind::UnableToDecrypt(_) => ReplyContent::Error("Unable to decrypt".into()),
                MsgLikeKind::Other(other) => ReplyContent::System(other.event_type().to_string()),
            }
        }
        TimelineItemContent::OtherState(other) => {
            ReplyContent::System(other.state_key().to_string())
        }
        TimelineItemContent::ProfileChange(_) => ReplyContent::System("Profile change".into()),
        TimelineItemContent::RtcNotification { call_intent, .. } => {
            ReplyContent::RtcNotification(if let Some(intent) = call_intent {
                format!("{} call", intent)
            } else {
                "Call".into()
            })
        }
    };

    ConvertTimelineReplyResult(
        next_data,
        ReplyToDetails {
            event_id,
            event: TimelineDetails::Ready(ReplyEvent {
                sender: embedded.sender.clone(),
                sender_profile: match &embedded.sender_profile {
                    TimelineDetails::Error(e) => TimelineDetails::Error(e.clone()),
                    TimelineDetails::Unavailable => TimelineDetails::Unavailable,
                    TimelineDetails::Pending => TimelineDetails::Pending,
                    TimelineDetails::Ready(p) => TimelineDetails::Ready(TimelineProfile {
                        display_name: p.display_name.clone(),
                        avatar_url: p.avatar_url.clone(),
                        user_id: embedded.sender.clone(),
                    }),
                },
                content,
            }),
        },
    )
}
