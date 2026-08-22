use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use deplace_core::{
    get_change,
    helpers::{
        DisplayString, format_date_divider, format_message_long_date, format_message_short_date,
        get_current_and_prev,
    },
};
use enumset::EnumSet;
use gpui::ImageFormat;
use matrix_sdk::{
    media::UniqueKey,
    ruma::{
        MilliSecondsSinceUnixEpoch, UserId,
        events::{
            receipt::ReceiptThread,
            room::message::{FormattedBody, MessageType},
            rtc::notification::CallIntent,
        },
    },
};
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange, BeaconInfo, EmbeddedEvent, EventSendState, InReplyToDetails,
    MembershipChange, MsgLikeKind, OtherState, TimelineDetails, TimelineItem, TimelineItemContent,
    TimelineItemKind, VirtualTimelineItem,
};

use super::{
    CachedBeaconInfo, CachedEventContent, CachedMediaUploadProgress, CachedMessageType,
    CachedProgress, CachedReplyInfo, CachedReplyPreview, CachedReplyPreviewBody, CachedSendState,
    CachedSystemMessage, CachedTimelineEvent, CachedTimelineItem, CachedTimelineItemKind,
    CachedUserMessage, DetailState, EventFlags, ReactionInfo,
    text::{CachedBlock, convert_formatted_body},
};
use crate::components::{ByteSize, blurhash_to_image};

impl From<&EventSendState> for CachedSendState {
    fn from(state: &EventSendState) -> Self {
        match state {
            EventSendState::SendingFailed {
                error,
                is_recoverable,
            } => CachedSendState::SendingFailed {
                error: error.to_string().into(),
                is_recoverable: *is_recoverable,
            },
            EventSendState::Sent { event_id } => CachedSendState::Sent {
                event_id: Arc::new(event_id.clone()),
            },
            EventSendState::NotSentYet { progress } => {
                let cached_progress = progress.clone().map(|p| CachedMediaUploadProgress {
                    index: p.index,
                    progress: CachedProgress {
                        current: p.progress.current,
                        total: p.progress.total,
                    },
                });
                CachedSendState::NotSentYet {
                    progress: cached_progress,
                }
            }
        }
    }
}

impl From<&BeaconInfo> for CachedBeaconInfo {
    fn from(beacon_info: &BeaconInfo) -> Self {
        Self {
            geo_uri: beacon_info.geo_uri().into(),
            description: beacon_info.description().map(|d| d.into()),
            timestamp: beacon_info.ts().as_secs().into(),
        }
    }
}

impl From<&OtherState> for CachedSystemMessage {
    fn from(state: &OtherState) -> Self {
        match state.content() {
            AnyOtherStateEventContentChange::PolicyRuleRoom(_) => {
                CachedSystemMessage::PolicyRuleRoom("changed the room's policy".into())
            }
            AnyOtherStateEventContentChange::PolicyRuleServer(_) => {
                CachedSystemMessage::PolicyRuleServer("changed the server's policy".into())
            }
            AnyOtherStateEventContentChange::PolicyRuleUser(_) => {
                CachedSystemMessage::PolicyRuleUser("changed their policy".into())
            }
            AnyOtherStateEventContentChange::RoomAvatar(content) => {
                let change = get_change!(content, |c| c.url.clone());
                CachedSystemMessage::RoomAvatar(change.display_string("the room's avatar").into())
            }
            AnyOtherStateEventContentChange::RoomCanonicalAlias(content) => {
                let change = get_change!(content, |c| c.alias.clone());
                CachedSystemMessage::RoomCanonicalAlias(
                    change.display_string("the room's canonical alias").into(),
                )
            }
            AnyOtherStateEventContentChange::RoomCreate(_) => {
                CachedSystemMessage::RoomCreate("created the room".into())
            }
            AnyOtherStateEventContentChange::RoomEncryption(_) => {
                CachedSystemMessage::RoomEncryption("enabled room encryption".into())
            }
            AnyOtherStateEventContentChange::RoomGuestAccess(content) => {
                let change = get_current_and_prev(
                    content,
                    |c| Some(c.guest_access.clone()),
                    |c| c.guest_access.clone(),
                );
                CachedSystemMessage::RoomGuestAccess(
                    change.display_string("the room's guest access").into(),
                )
            }
            AnyOtherStateEventContentChange::RoomHistoryVisibility(content) => {
                let change = get_change!(content, |c| Some(c.history_visibility.clone()));
                CachedSystemMessage::RoomHistoryVisibility(
                    change
                        .display_string("the room's history visibility")
                        .into(),
                )
            }
            AnyOtherStateEventContentChange::RoomJoinRules(content) => {
                let change = get_change!(content, |c| Some(c.join_rule.clone()));
                CachedSystemMessage::RoomJoinRules(
                    change
                        .display_string_with_render_fn(
                            |c| c.as_str().to_string(),
                            "the room's join rules",
                        )
                        .into(),
                )
            }
            AnyOtherStateEventContentChange::RoomName(content) => {
                let change =
                    get_current_and_prev(content, |c| Some(c.name.clone()), |c| c.name.clone());
                CachedSystemMessage::RoomName(change.display_string("the room's name").into())
            }
            AnyOtherStateEventContentChange::RoomPinnedEvents(_) => {
                CachedSystemMessage::RoomPinnedEvents("changed the room's pinned events".into())
            }
            AnyOtherStateEventContentChange::RoomPowerLevels(_) => {
                CachedSystemMessage::RoomPowerLevels("changed the room's power levels".into())
            }
            AnyOtherStateEventContentChange::RoomServerAcl(_) => {
                CachedSystemMessage::RoomServerAcl("changed the room's server ACL".into())
            }
            AnyOtherStateEventContentChange::RoomThirdPartyInvite(_) => {
                CachedSystemMessage::RoomThirdPartyInvite(
                    "changed the room's third party invite".into(),
                )
            }
            AnyOtherStateEventContentChange::RoomTombstone(_) => {
                CachedSystemMessage::RoomTombstone("changed the room's tombstone".into())
            }
            AnyOtherStateEventContentChange::RoomTopic(content) => {
                let change =
                    get_current_and_prev(content, |c| Some(c.topic.clone()), |c| c.topic.clone());
                CachedSystemMessage::RoomTopic(change.display_string("the room's topic").into())
            }
            AnyOtherStateEventContentChange::SpaceChild(_) => {
                CachedSystemMessage::SpaceChild("changed the space's child".into())
            }
            AnyOtherStateEventContentChange::SpaceParent(_) => {
                CachedSystemMessage::SpaceParent("changed the space's parent".into())
            }
            _ => {
                let state_key = state.state_key();
                if state_key.starts_with("_") {
                    CachedSystemMessage::Invisible
                } else {
                    CachedSystemMessage::Unknown(
                        format!("sent an {} event", state.state_key()).into(),
                    )
                }
            }
        }
    }
}

impl CachedTimelineItem {
    pub fn recompute_datedivider_types(&mut self, rest: &[CachedTimelineItem]) {
        let CachedTimelineItemKind::DateDivider {
            depends_on_system_messages,
            ..
        } = &mut self.kind
        else {
            return;
        };

        *depends_on_system_messages = None;

        let mut types = EnumSet::empty();
        for item in rest {
            if let CachedTimelineItemKind::DateDivider { .. } = item.kind {
                break;
            }

            let CachedTimelineItemKind::Event(event) = &item.kind else {
                return;
            };
            let Some(sys) = event.as_system_message() else {
                return;
            };

            types.insert(sys.message_type());
        }

        *depends_on_system_messages = Some(types);
    }
}

pub fn cached_from_timeline_item(value: &Arc<TimelineItem>, own_id: &UserId) -> CachedTimelineItem {
    let kind = match value.kind() {
        TimelineItemKind::Virtual(virt) => match virt {
            VirtualTimelineItem::DateDivider(ts) => {
                let secs = ts.as_secs().into();
                let date = Local
                    .timestamp_opt(secs, 0)
                    .latest()
                    .unwrap_or_else(|| DateTime::UNIX_EPOCH.with_timezone(&Local));
                let text = format_date_divider(date);

                CachedTimelineItemKind::DateDivider {
                    date: text.into(),
                    depends_on_system_messages: None,
                }
            }
            VirtualTimelineItem::ReadMarker => CachedTimelineItemKind::ReadMarker,
            VirtualTimelineItem::TimelineStart => CachedTimelineItemKind::TimelineStart,
        },
        TimelineItemKind::Event(event) => {
            let sender = Arc::new(event.sender().to_owned());

            let read_by = Arc::new(
                event
                    .read_receipts()
                    .iter()
                    .filter_map(|(user_id, receipt)| {
                        if matches!(
                            receipt.thread,
                            ReceiptThread::Main | ReceiptThread::Unthreaded
                        ) {
                            Some(user_id.to_owned())
                        } else {
                            None
                        }
                    })
                    .collect(),
            );

            let secs = event.timestamp().as_secs().into();
            let date = Local
                .timestamp_opt(secs, 0)
                .latest()
                .unwrap_or_else(|| DateTime::UNIX_EPOCH.with_timezone(&Local));

            let event_content = event.content();
            let mut cached_event = CachedTimelineEvent {
                state: event.send_state().map(|s| s.into()),
                flags: EventFlags {
                    is_reactable: true,
                    is_deletable: event.is_own(),
                    is_editable: event.is_editable(),
                    is_highlighted: event.is_highlighted(),
                    can_be_replied_to: event.can_be_replied_to(),
                    contains_only_emojis: event.contains_only_emojis(),
                },
                short_time: format_message_short_date(date).into(),
                long_time: format_message_long_date(date).into(),
                timestamp: secs,
                event_id: event.event_id().map(|id| Arc::new(id.to_owned())),
                sender,
                read_by,
                show_header: true,
                pad_bottom: false,
                content: cached_from_timeline_item_content(event_content, own_id),
            };

            cached_event.calculate_flags(event.is_own(), event_content.is_redacted());

            CachedTimelineItemKind::Event(Box::new(cached_event))
        }
    };

    CachedTimelineItem {
        kind,
        id: value.unique_id().clone().0.into(),
    }
}

fn convert_to_formatted(body: &str, formated: Option<&FormattedBody>) -> Arc<[CachedBlock]> {
    if let Some(formatted) = formated {
        convert_formatted_body(&formatted.body)
    } else {
        Arc::new([CachedBlock::new_plain(body)])
    }
}

fn cached_from_timeline_item_content(
    value: &TimelineItemContent,
    own_id: &UserId,
) -> CachedEventContent {
    match value {
        TimelineItemContent::CallInvite => CachedEventContent::SystemMessage(
            CachedSystemMessage::CallInvite("invited to a call".into()),
        ),
        TimelineItemContent::FailedToParseMessageLike { event_type, error } => {
            CachedEventContent::FailedToParseMessageLike(
                format!("Failed to parse event of type {event_type}: {error}").into(),
            )
        }
        TimelineItemContent::FailedToParseState {
            event_type,
            state_key,
            error,
        } => CachedEventContent::FailedToParseState(
            format!("Failed to parse state of type {event_type} with key {state_key}: {error}")
                .into(),
        ),
        TimelineItemContent::MembershipChange(change) => {
            CachedEventContent::SystemMessage(CachedSystemMessage::MemberShipChange(
                if let Some(membership) = &change.change() {
                    match membership {
                        MembershipChange::None => "had no membership change",
                        MembershipChange::Banned => "was banned",
                        MembershipChange::Joined => "joined the room",
                        MembershipChange::Invited => "was invited",
                        MembershipChange::Left => "left the room",
                        MembershipChange::Kicked => "was kicked",
                        MembershipChange::Error => "had a membership change",
                        MembershipChange::InvitationAccepted => "accepted the invitation",
                        MembershipChange::InvitationRejected => "rejected the invitation",
                        MembershipChange::InvitationRevoked => "had their invitation revoked",
                        MembershipChange::KickedAndBanned => "was kicked and banned",
                        MembershipChange::KnockAccepted => "accepted the knock",
                        MembershipChange::KnockDenied => "denied the knock",
                        MembershipChange::KnockRetracted => "retracted the knock",
                        MembershipChange::Knocked => "knocked on the door",
                        MembershipChange::NotImplemented => "had a membership change",
                        MembershipChange::Unbanned => "was unbanned",
                    }
                } else {
                    "changed membership"
                }
                .into(),
            ))
        }
        TimelineItemContent::OtherState(other) => CachedEventContent::SystemMessage(other.into()),
        TimelineItemContent::ProfileChange(change) => CachedEventContent::SystemMessage(
            CachedSystemMessage::ProfileChange(change.display_string().into()),
        ),
        TimelineItemContent::RtcNotification {
            call_intent,
            declined_by,
        } => CachedEventContent::SystemMessage(CachedSystemMessage::RtcNotification {
            text: gpui::SharedString::from(format!(
                "{}{}",
                if let Some(intent) = call_intent {
                    match intent {
                        CallIntent::Video => "started a video call",
                        CallIntent::Audio | _ => "started an audio call",
                    }
                } else {
                    "started a call"
                },
                if !declined_by.is_empty() {
                    ", declined by"
                } else {
                    ""
                }
            )),
            declined_by: declined_by.iter().map(|d| Arc::new(d.to_owned())).collect(),
        }),
        TimelineItemContent::MsgLike(msg_like) => {
            let mut is_edited = false;

            let (mut body, mut msg_type) = match &msg_like.kind {
                MsgLikeKind::LiveLocation(loc) => (
                    None,
                    CachedMessageType::LiveLocation {
                        locations: loc.locations().iter().map(|l| l.into()).collect(),
                    },
                ),
                MsgLikeKind::Message(msg) => {
                    is_edited = msg.is_edited();

                    match msg.msgtype().clone() {
                        MessageType::Audio(content) => (
                            Some(convert_to_formatted(
                                &content.body,
                                content.formatted.as_ref(),
                            )),
                            CachedMessageType::Audio {
                                _source: Arc::new(content.source.clone()),
                                _filename: content.filename().into(),
                                _duration: content
                                    .info
                                    .map(|v| v.duration.map(|d| d.as_secs()))
                                    .unwrap_or_default(),
                            },
                        ),
                        MessageType::Emote(content) => (
                            Some(convert_to_formatted(
                                &content.body,
                                content.formatted.as_ref(),
                            )),
                            CachedMessageType::Emote,
                        ),
                        MessageType::File(content) => {
                            let info = content.info.clone().unwrap_or_default();
                            let filename = content.filename();

                            (
                                (filename != content.body).then_some(convert_to_formatted(
                                    &content.body,
                                    content.formatted.as_ref(),
                                )),
                                CachedMessageType::File {
                                    source: content.source.clone().into(),
                                    filename: content.filename().into(),
                                    _mime_type: info.mimetype.map(|m| m.into()),
                                    size: info.size.map(|s| ByteSize::new(s.into())),
                                },
                            )
                        }
                        MessageType::Image(content) => {
                            let info = content.info.clone().unwrap_or_default();
                            let filename = content.filename();

                            let format =
                                info.mimetype.and_then(|m| ImageFormat::from_mime_type(&m));

                            (
                                (filename != content.body).then_some(convert_to_formatted(
                                    &content.body,
                                    content.formatted.as_ref(),
                                )),
                                CachedMessageType::Image {
                                    filename: filename.into(),
                                    source: Arc::new(content.source.clone()),
                                    source_key: content.source.unique_key().into(),
                                    width: info.width.map(|w| u64::from(w) as f32),
                                    height: info.height.map(|h| u64::from(h) as f32),
                                    size: info.size.map(|s| ByteSize::new(s.into())),
                                    format,
                                    blurhash_image: info
                                        .blurhash
                                        .and_then(|h| blurhash_to_image(&h)),
                                },
                            )
                        }
                        MessageType::Location(content) => (
                            Some(convert_to_formatted(&content.body, None)),
                            CachedMessageType::Location(CachedBeaconInfo {
                                geo_uri: content.geo_uri.into(),
                                description: content
                                    .message
                                    .map(|v| v.find_plain().map(|p| p.into()).unwrap_or_default()),
                                timestamp: content
                                    .ts
                                    .unwrap_or(MilliSecondsSinceUnixEpoch::now())
                                    .as_secs()
                                    .into(),
                            }),
                        ),
                        MessageType::Notice(content) => (
                            Some(convert_to_formatted(
                                &content.body,
                                content.formatted.as_ref(),
                            )),
                            CachedMessageType::Notice,
                        ),
                        MessageType::ServerNotice(content) => (
                            Some(convert_to_formatted(&content.body, None)),
                            CachedMessageType::ServerNotice {
                                admin_contact: content.admin_contact.map(|c| c.into()),
                            },
                        ),
                        MessageType::Text(content) => (
                            Some(convert_to_formatted(
                                &content.body,
                                content.formatted.as_ref(),
                            )),
                            CachedMessageType::Text,
                        ),
                        MessageType::Video(content) => {
                            let info = content.info.clone().unwrap_or_default();
                            let filename = content.filename();

                            (
                                (filename != content.body).then_some(convert_to_formatted(
                                    &content.body,
                                    content.formatted.as_ref(),
                                )),
                                CachedMessageType::Video {
                                    source: content.source.clone().into(),
                                    filename: filename.into(),
                                    width: info.width.map(|w| w.into()),
                                    height: info.height.map(|h| h.into()),
                                    duration: info.duration.map(|d| d.as_secs()),
                                    size: info.size.map(|s| s.into()),
                                    mime_type: info.mimetype.map(|m| m.into()),
                                    blurhash: info.blurhash.map(|h| h.into()),
                                },
                            )
                        }
                        _ => (
                            value
                                .as_message()
                                .map(|m| convert_to_formatted(m.body(), None)),
                            CachedMessageType::Text,
                        ),
                    }
                }
                MsgLikeKind::Other(other) => (
                    None,
                    CachedMessageType::Other {
                        msg_type: other.event_type().to_string().into(),
                    },
                ),
                MsgLikeKind::Poll(_) => (None, CachedMessageType::Poll),
                MsgLikeKind::Redacted => (None, CachedMessageType::Redacted),
                MsgLikeKind::Sticker(_) => (None, CachedMessageType::Sticker),
                MsgLikeKind::UnableToDecrypt(_) => (None, CachedMessageType::UnableToDecrypt),
            };

            if let Some(ref text) = body
                && text.is_empty()
            {
                body = None;

                if matches!(msg_type, CachedMessageType::Text) {
                    msg_type = CachedMessageType::Empty;
                }
            }

            let reactions = value.reactions().map(|reactions| {
                let mut reactions: Vec<ReactionInfo> = reactions
                    .iter()
                    .map(|(reaction, senders)| {
                        let mut timestamp = u64::MAX;
                        let mut has_own = false;
                        let mut reactors = Vec::new();

                        for (id, info) in senders {
                            let ts = info.timestamp.as_secs().into();
                            if ts < timestamp {
                                timestamp = ts;
                            }
                            if id == own_id {
                                has_own = true;
                            }
                            reactors.push((id.clone(), ts));
                        }

                        reactors.sort_by_key(|(_, ts)| *ts);

                        ReactionInfo {
                            reactors_count: reactors.len().to_string().into(),
                            reactors: Arc::new(reactors.into_iter().map(|(id, _)| id).collect()),
                            emoji: reaction.into(),
                            timestamp,
                            has_own,
                        }
                    })
                    .collect();

                reactions.sort_by_key(|info| info.timestamp);
                reactions
            });

            let in_reply_to = value.in_reply_to().map(|details| details.into());

            CachedEventContent::UserMessage(Box::new(CachedUserMessage {
                reactions,
                in_reply_to,
                is_edited,
                body,
                msg_type,
                editing: None,
            }))
        }
    }
}

impl From<InReplyToDetails> for CachedReplyInfo {
    fn from(details: InReplyToDetails) -> Self {
        let body = match details.event {
            TimelineDetails::Error(error) => DetailState::Error(error.to_string().into()),
            TimelineDetails::Pending => DetailState::Pending,
            TimelineDetails::Unavailable => DetailState::Unavailable,
            TimelineDetails::Ready(ev) => DetailState::Ready(ev.into()),
        };

        CachedReplyInfo {
            event_id: Arc::new(details.event_id),
            body,
        }
    }
}

impl From<Box<EmbeddedEvent>> for CachedReplyPreview {
    fn from(event: Box<EmbeddedEvent>) -> Self {
        let body = match event.content {
            TimelineItemContent::CallInvite => CachedReplyPreviewBody::CallInvite,
            TimelineItemContent::FailedToParseMessageLike { .. } => {
                CachedReplyPreviewBody::Error("Failed to parse message".into())
            }
            TimelineItemContent::FailedToParseState { .. } => {
                CachedReplyPreviewBody::Error("Failed to parse state".into())
            }
            TimelineItemContent::MembershipChange(_) => {
                CachedReplyPreviewBody::System("Membership change".into())
            }
            TimelineItemContent::MsgLike(msglike) => match msglike.kind {
                MsgLikeKind::LiveLocation(_) => CachedReplyPreviewBody::Location,
                MsgLikeKind::Poll(_) => CachedReplyPreviewBody::Poll,
                MsgLikeKind::Message(msg) => match msg.msgtype() {
                    MessageType::Audio(_) => CachedReplyPreviewBody::Audio,
                    MessageType::Emote(content) => {
                        CachedReplyPreviewBody::Emote(content.body.as_str().into())
                    }
                    MessageType::File(_) | MessageType::Video(_) | MessageType::Image(_) => {
                        CachedReplyPreviewBody::Media
                    }
                    MessageType::Location(_) => CachedReplyPreviewBody::Location,
                    MessageType::Notice(content) => {
                        CachedReplyPreviewBody::Text(content.body.as_str().into())
                    }
                    MessageType::ServerNotice(content) => {
                        CachedReplyPreviewBody::Text(content.body.as_str().into())
                    }
                    MessageType::Text(content) => {
                        CachedReplyPreviewBody::Text(content.body.as_str().into())
                    }
                    MessageType::VerificationRequest(_) => {
                        CachedReplyPreviewBody::Text("Verification request".into())
                    }
                    _ => CachedReplyPreviewBody::Error("Unknown message type".into()),
                },
                MsgLikeKind::Redacted => CachedReplyPreviewBody::Redacted,
                MsgLikeKind::Sticker(_) => CachedReplyPreviewBody::Sticker,
                MsgLikeKind::UnableToDecrypt(_) => {
                    CachedReplyPreviewBody::Error("Unable to decrypt".into())
                }
                MsgLikeKind::Other(other) => {
                    CachedReplyPreviewBody::System(other.event_type().to_string().into())
                }
            },
            TimelineItemContent::OtherState(other) => {
                CachedReplyPreviewBody::System(other.state_key().to_string().into())
            }
            TimelineItemContent::ProfileChange(_) => CachedReplyPreviewBody::ProfileChange,
            TimelineItemContent::RtcNotification { call_intent, .. } => {
                CachedReplyPreviewBody::RtcNotification(if let Some(intent) = call_intent {
                    format!("{} call", intent).into()
                } else {
                    "Call".into()
                })
            }
        };

        Self {
            sender_id: Arc::new(event.sender),
            body,
        }
    }
}
