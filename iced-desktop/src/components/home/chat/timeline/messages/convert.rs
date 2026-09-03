use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::common::*;
use crate::components::home::chat::timeline::messages::TimelineProfile;

use super::{MessageContent, MessageEvent, SystemEvent, SystemMessage};
use super::{TimelineItem, TimelineItemKind};

use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk_ui::timeline::TimelineItemKind as UiTimelineItemKind;
use matrix_sdk_ui::timeline::VirtualTimelineItem;
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent};
use matrix_sdk_ui::timeline::{TimelineDetails, TimelineItem as UiTimelineItem};

pub trait ToTimelineItem {
    fn convert(self, avatar_cache: &AvatarCache, thumbnail_cache: &ThumbnailCache) -> TimelineItem;
}

impl ToTimelineItem for Arc<UiTimelineItem> {
    fn convert(self, avatar_cache: &AvatarCache, thumbnail_cache: &ThumbnailCache) -> TimelineItem {
        TimelineItem {
            id: self.unique_id().0.clone(),
            kind: TimelineItemKind::from_ui(self.kind()),

            avatar_cache: avatar_cache.clone(),
            avatar_states_for_hash: BTreeSet::new(),
            thumbnail_cache: thumbnail_cache.clone(),
            thumbnail_states_for_hash: BTreeSet::new(),
        }
    }
}

impl TimelineItemKind {
    fn from_ui(value: &UiTimelineItemKind) -> Self {
        match value {
            UiTimelineItemKind::Virtual(virt) => match virt {
                VirtualTimelineItem::ReadMarker => TimelineItemKind::ReadMarker,
                VirtualTimelineItem::TimelineStart => TimelineItemKind::TimelineStart,
                VirtualTimelineItem::DateDivider(ms) => TimelineItemKind::DateDivider(
                    ms.to_system_time().unwrap_or_else(SystemTime::now),
                ),
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
                        TimelineItemKind::System(Arc::new(SystemEvent {
                            timestamp,
                            event_id,
                            sender,
                            sender_profile,
                            content: $content,
                        }))
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
                        system!(SystemMessage::MemberhipChange(c.clone()))
                    }
                    TimelineItemContent::OtherState(o) => {
                        system!(SystemMessage::OtherState(o.clone()))
                    }
                    TimelineItemContent::ProfileChange(p) => {
                        system!(SystemMessage::ProfileChange(p.clone()))
                    }
                    TimelineItemContent::RtcNotification {
                        call_intent,
                        declined_by,
                    } => system!(SystemMessage::RtcNotification {
                        call_intent: call_intent.clone(),
                        declined_by: declined_by.clone(),
                    }),
                    TimelineItemContent::MsgLike(m) => {
                        TimelineItemKind::Message(Box::new(MessageEvent {
                            timestamp,

                            event_id,
                            sender,
                            sender_profile,

                            media_hovered: false,

                            in_reply_to: Arc::new(Vec::new()),

                            reactions: m.reactions.clone(),

                            is_own: event.is_own(),
                            is_editable: event.is_editable(),
                            is_highlighted: event.is_highlighted(),
                            can_be_replied_to: event.can_be_replied_to(),
                            contains_only_emojis: event.contains_only_emojis(),

                            shield: event.get_shield(false),
                            send_state: event.send_state().cloned(),

                            content: Arc::new(MessageContent::from(&m.kind)),
                        }))
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

impl From<&MsgLikeKind> for MessageContent {
    fn from(value: &MsgLikeKind) -> Self {
        match value {
            MsgLikeKind::Message(msg) => match msg.msgtype() {
                MessageType::Audio(_) => MessageContent::Audio,
                MessageType::Emote(emote) => MessageContent::Emote {
                    body: string_to_option(&emote.body),
                    formatted_body: emote.formatted.clone(),
                },
                MessageType::File(file) => MessageContent::File {
                    caption: file.caption().map(|s| s.to_string()),
                    formatted_caption: file.formatted_caption().cloned(),
                    filename: file.filename().to_string(),
                    source: file.source.clone(),
                    info: file.info.clone(),
                },
                MessageType::Image(image) => MessageContent::Image {
                    caption: image.caption().map(|s| s.to_string()),
                    formatted_caption: image.formatted_caption().cloned(),
                    filename: image.filename().to_string(),
                    source: image.source.clone(),
                    info: image.info.clone(),
                },
                MessageType::Location(_) => MessageContent::Location,
                MessageType::Notice(notice) => MessageContent::Notice {
                    body: string_to_option(&notice.body),
                    formatted_body: notice.formatted.clone(),
                },
                MessageType::ServerNotice(server_notice) => MessageContent::ServerNotice {
                    body: string_to_option(&server_notice.body),
                },
                MessageType::Text(text) => MessageContent::Text {
                    body: string_to_option(&text.body),
                    formatted_body: text.formatted.clone(),
                    _url_previews: text.url_previews.clone(),
                },
                MessageType::VerificationRequest(request) => MessageContent::VerificationRequest {
                    body: string_to_option(&request.body),
                    formatted_body: request.formatted.clone(),
                },
                MessageType::Video(video) => MessageContent::Video {
                    caption: video.caption().map(|s| s.to_string()),
                    formatted_caption: video.formatted_caption().cloned(),
                    filename: video.filename().to_string(),
                    source: video.source.clone(),
                    info: video.info.clone(),
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
