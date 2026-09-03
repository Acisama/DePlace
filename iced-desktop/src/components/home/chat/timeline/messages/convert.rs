use std::collections::BTreeSet;
use std::time::SystemTime;

use crate::common::*;
use crate::components::home::chat::timeline::messages::TimelineProfile;

use super::{MessageEvent, SystemEvent, SystemMessage};
use super::{TimelineItem, TimelineItemKind};

use matrix_sdk_ui::timeline::TimelineItemContent;
use matrix_sdk_ui::timeline::TimelineItemKind as UiTimelineItemKind;
use matrix_sdk_ui::timeline::VirtualTimelineItem;
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
                            event_type: event_type.to_string(),
                            error: error.clone(),
                        }
                    }
                    TimelineItemContent::FailedToParseState {
                        event_type,
                        state_key,
                        error,
                    } => TimelineItemKind::FailedToParseState {
                        event_type: event_type.to_string(),
                        state_key: state_key.to_string(),
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

                            in_reply_to: Arc::new(Vec::new()),

                            reactions: m.reactions.clone(),

                            is_own: event.is_own(),
                            is_editable: event.is_editable(),
                            is_highlighted: event.is_highlighted(),
                            can_be_replied_to: event.can_be_replied_to(),
                            contains_only_emojis: event.contains_only_emojis(),

                            shield: event.get_shield(false),
                            send_state: event.send_state().cloned(),

                            content: m.kind.clone(),
                        }))
                    }
                }
            }
        }
    }
}
