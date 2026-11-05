use deplace_core::rich_text::FormattedBody;
use matrix_sdk::ruma::events::StateEventType;
use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::common::*;
use crate::components::{blurhash_to_image, thumbhash_to_image};
use crate::modules::home::chat::timeline::messages::RtcNotification;

use super::{
    ImageMessage, MessageContent, MessageEvent, ReactionInfoMap, ReplyContent, ReplyEvent,
    ReplyToDetails, SystemEvent, SystemMessage, VideoMessage, VisualInfo,
};
use super::{TimelineItem, TimelineItemKind};

use matrix_sdk::ruma::events::room::ImageInfo;
use matrix_sdk::ruma::events::room::message::{MessageType, VideoInfo};
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange, EmbeddedEvent, ReactionsByKeyBySender,
    TimelineItemKind as UiTimelineItemKind,
};
use matrix_sdk_ui::timeline::{InReplyToDetails, VirtualTimelineItem};
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent};
use matrix_sdk_ui::timeline::{TimelineDetails, TimelineItem as UiTimelineItem};

pub trait ToTimelineItem {
    fn convert(
        self,
        state: &AppState,
        profile_store: Arc<ProfileStore>,
        room_id: OwnedRoomId,
        pinned_event_ids: &BTreeSet<OwnedEventId>,
        own_user_id: &OwnedUserId,
    ) -> TimelineItem;
}

impl ToTimelineItem for Arc<UiTimelineItem> {
    fn convert(
        self,
        state: &AppState,
        profile_store: Arc<ProfileStore>,
        room_id: OwnedRoomId,
        pinned_event_ids: &BTreeSet<OwnedEventId>,
        own_user_id: &OwnedUserId,
    ) -> TimelineItem {
        TimelineItem {
            id: self.unique_id().0.clone(),
            kind: TimelineItemKind::from_ui(
                self.kind(),
                state,
                profile_store,
                pinned_event_ids,
                &room_id,
                own_user_id,
            ),
            room_id,
        }
    }
}

impl TimelineItemKind {
    fn from_ui(
        value: &UiTimelineItemKind,
        state: &AppState,
        profile_store: Arc<ProfileStore>,
        pinned_event_ids: &BTreeSet<OwnedEventId>,
        room_id: &OwnedRoomId,
        own_user_id: &OwnedUserId,
    ) -> Self {
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

                let get_member = |id: &UserId| profile_store.get_member(room_id, id);

                let sender = get_member(event.sender());
                let event_id = event.event_id().map(|id| id.to_owned());
                let identifier = event.identifier();

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
                                _event_id: event_id,
                                sender,
                                content: Arc::new($content),

                                timezone: state.settings().timezone.watch(),
                                hour_format: state.settings().hour_format.watch(),
                                date_format: state.settings().date_format.watch(),
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
                    } => {
                        let mut user_is_in_call = false;
                        let active_members = active_call_info.as_ref().map(|info| {
                            info.active_members
                                .iter()
                                .map(|id| {
                                    if id == own_user_id {
                                        user_is_in_call = true;
                                    }
                                    get_member(id)
                                })
                                .collect()
                        });

                        system!(SystemMessage::RtcNotification(RtcNotification {
                            user_is_in_call,
                            call_intent: call_intent.clone(),
                            call_started: active_call_info
                                .as_ref()
                                .and_then(|info| info.call_started_ts_millis)
                                .and_then(|t| t.to_system_time()),
                            active_members,
                            declined_by_user: declined_by.iter().any(|id| id == own_user_id),
                        }))
                    }
                    TimelineItemContent::MsgLike(m) => {
                        let settings = state.settings();
                        let content = MessageContent::from_ui(&m.kind, state);
                        let copyable_text = content.copyable_text().map(Arc::new);

                        TimelineItemKind::Message {
                            is_own: event.is_own(),
                            is_editable: event.is_editable(),
                            can_be_replied_to: event.can_be_replied_to(),
                            message: Arc::new(MessageEvent {
                                timestamp,
                                identifier,

                                timezone: settings.timezone.watch(),
                                hour_format: settings.hour_format.watch(),
                                date_format: settings.date_format.watch(),

                                sender,

                                is_replying_to: false,

                                reactions: convert_reactions(
                                    &m.reactions,
                                    own_user_id,
                                    &profile_store,
                                    room_id,
                                ),

                                in_reply_to: Arc::new(
                                    m.in_reply_to
                                        .as_ref()
                                        .map(|e| get_reply_details(e, &profile_store, room_id))
                                        .unwrap_or_default(),
                                ),
                                profile_store,

                                is_pinned: event_id
                                    .as_ref()
                                    .is_some_and(|id| pinned_event_ids.contains(id)),
                                is_edited: m.as_message().map(|m| m.is_edited()).unwrap_or(false),

                                event_id,

                                connects_previous: false,

                                is_highlighted: event.is_highlighted(),
                                contains_only_emojis: event.contains_only_emojis(),

                                _shield: event.get_shield(false),
                                send_state: event.send_state().cloned(),

                                content,
                                copyable_text,
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

fn convert_reactions(
    reactions: &ReactionsByKeyBySender,
    own_user_id: &UserId,
    profile_store: &ProfileStore,
    room_id: &RoomId,
) -> Option<ReactionInfoMap> {
    if reactions.is_empty() {
        return None;
    }

    let mut reactions: IndexMap<String, (bool, Vec<Member>, Option<SystemTime>)> = reactions
        .iter()
        .map(|(reaction, reactors)| {
            let mut oldest_time = None;
            let mut contains_own = false;

            let mut reactors: IndexMap<Member, SystemTime> = reactors
                .iter()
                .map(|(id, info)| {
                    let timestamp = info.timestamp.to_system_time();

                    if timestamp > oldest_time {
                        oldest_time = timestamp;
                    }
                    if id == own_user_id {
                        contains_own = true;
                    }

                    (
                        profile_store.get_member(room_id, id),
                        timestamp.unwrap_or(SystemTime::now()),
                    )
                })
                .collect();

            reactors.sort_by_key(|_, t| *t);

            (
                reaction.clone(),
                (
                    contains_own,
                    reactors.into_iter().map(|(m, _)| m).collect(),
                    oldest_time,
                ),
            )
        })
        .collect();

    reactions.sort_by_key(|_, (_, _, t)| *t);

    Some(Arc::new(
        reactions
            .into_iter()
            .map(|(k, (b, v, _))| (k, (b, v)))
            .collect(),
    ))
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

        let result = match value {
            MsgLikeKind::Message(msg) => match msg.msgtype() {
                MessageType::Audio(_) => MessageContent::Audio,
                MessageType::Emote(emote) => MessageContent::Emote {
                    body: string_to_option(&emote.body),
                    formatted_body: FormattedBody::new(Some(&emote.body), emote.formatted.as_ref()),
                },
                MessageType::File(file) => MessageContent::File {
                    caption: file.caption().map(|s| s.to_string()),
                    formatted_caption: FormattedBody::new(file.caption(), file.formatted_caption()),
                    filename: file.filename().to_string(),
                    source: file.source.clone(),
                    info: file.info.clone(),
                    data_size_unit: settings.data_size_unit.watch(),
                },
                MessageType::Image(image) => MessageContent::Image {
                    image: ImageMessage {
                        caption: image.caption().map(|s| s.to_string()),
                        formatted_caption: FormattedBody::new(
                            image.caption(),
                            image.formatted_caption(),
                        ),

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
                    formatted_body: FormattedBody::new(
                        Some(&notice.body),
                        notice.formatted.as_ref(),
                    ),
                },
                MessageType::ServerNotice(server_notice) => MessageContent::ServerNotice {
                    body: string_to_option(&server_notice.body),
                },
                MessageType::Text(text) => MessageContent::Text {
                    body: string_to_option(&text.body),
                    formatted_body: FormattedBody::new(Some(&text.body), text.formatted.as_ref()),
                    _url_previews: text.url_previews.clone(),
                },
                MessageType::VerificationRequest(request) => MessageContent::VerificationRequest {
                    body: string_to_option(&request.body),
                    formatted_body: FormattedBody::new(
                        Some(&request.body),
                        request.formatted.as_ref(),
                    ),
                },
                MessageType::Video(video) => MessageContent::Video {
                    video: VideoMessage {
                        caption: video.caption().map(|s| s.to_string()),
                        formatted_caption: FormattedBody::new(
                            video.caption(),
                            video.formatted_caption(),
                        ),

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
        };

        if matches!(&result, MessageContent::Text { body, .. } | MessageContent::Emote { body, .. } if body.is_none())
        {
            return MessageContent::Empty;
        }
        result
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

fn get_reply_details(
    event: &InReplyToDetails,
    profile_store: &ProfileStore,
    room_id: &OwnedRoomId,
) -> Vec<ReplyToDetails> {
    let mut replies = Vec::new();

    let mut current_event = event.event.clone();
    let mut current_id = event.event_id.clone();

    loop {
        let ConvertTimelineReplyResult(next_opt, converted) =
            convert_timeline_reply(&current_event, profile_store, current_id, room_id);

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
    profile_store: &ProfileStore,
    event_id: OwnedEventId,
    room_id: &OwnedRoomId,
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
                sender: profile_store.get_member(room_id, &embedded.sender),
                content,
            }),
        },
    )
}
