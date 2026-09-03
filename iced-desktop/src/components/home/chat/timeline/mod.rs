use std::{collections::BTreeSet, hash::Hasher};

use macros::iced_cache;
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk_ui::{Timeline, eyeball_im::VectorDiff, timeline::TimelineItem as UiTimelineItem};
use messages::{TimelineItem, TimelineItemAction, TimelineItemMessage};

use crate::common::*;

pub use messages::ToTimelineItem;
mod messages;

#[derive(Debug, Clone)]
pub enum TimelineMessage {
    Item {
        id: String,
        message: TimelineItemMessage,
    },
    NeedsAvatar(OwnedMxcUri),
    NeedsThumbnail {
        source: MediaSource,
        width: u64,
        height: u64,
    },
    Loaded {
        timeline: Arc<Timeline>,
        initial: Arc<IndexMap<String, TimelineItem>>,
    },
    Diffs(Vec<VectorDiff<Arc<UiTimelineItem>>>),
    SetReplying(OwnedEventId),
    None,
}

impl NeedsAvatarExt for TimelineMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        TimelineMessage::NeedsAvatar(uri)
    }
}

pub enum TimelineAction {
    SetReplying(OwnedEventId),
    Run(Task<()>),
    None,
}

#[iced_cache(Clone)]
pub struct ChatTimeline {
    timeline: Option<Arc<Timeline>>,

    avatar_cache: AvatarCache,
    thumbnail_cache: ThumbnailCache,

    #[hash]
    room_id: OwnedRoomId,

    messages: Arc<IndexMap<String, TimelineItem>>,
    #[hash]
    messages_version: u64,
}

impl ChatTimeline {
    pub fn new(room: &Room, state: &AppState) -> Self {
        Self {
            timeline: None,
            room_id: room.room_id().to_owned(),

            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),

            thumbnail_cache: state.thumbnail_cache().clone(),
            thumbnail_states_for_hash: BTreeSet::new(),

            messages: Arc::new(IndexMap::new()),
            messages_version: 0,
        }
    }
}

impl IcedWidget<TimelineMessage, TimelineAction> for ChatTimeline {
    fn update(&mut self, message: TimelineMessage) -> TimelineAction {
        match message {
            TimelineMessage::Item { id, message } => {
                let Some(item) = Arc::make_mut(&mut self.messages).get_mut(&id) else {
                    tracing::warn!("No item found for id {}", id);
                    return TimelineAction::None;
                };

                return match item.update(message) {
                    TimelineItemAction::None => TimelineAction::None,
                    TimelineItemAction::NeedsAvatar(uri) => {
                        TimelineAction::Run(self.retain_avatar_hashes(uri))
                    }
                    TimelineItemAction::NeedsThumbnail {
                        source,
                        width,
                        height,
                    } => TimelineAction::Run(self.retain_thumbnail_hashes(source, width, height)),
                };
            }
            TimelineMessage::NeedsAvatar(uri) => {
                return TimelineAction::Run(self.retain_avatar_hashes(uri));
            }
            TimelineMessage::NeedsThumbnail {
                source,
                width,
                height,
            } => {
                return TimelineAction::Run(self.retain_thumbnail_hashes(source, width, height));
            }
            TimelineMessage::Loaded { timeline, initial } => {
                tracing::debug!(
                    "ChatTimeline for room {} loaded with {} initial items",
                    self.room_id,
                    initial.len()
                );
                self.timeline = Some(timeline);
                self.messages = initial;
                self.messages_version += 1;
            }
            TimelineMessage::Diffs(diffs) => {
                tracing::debug!(
                    "ChatTimeline for room {} applying {} diff(s), {} messages before",
                    self.room_id,
                    diffs.len(),
                    self.messages.len()
                );

                let messages = Arc::make_mut(&mut self.messages);
                let avatar_cache = &self.avatar_cache;
                let thumbnail_cache = &self.thumbnail_cache;

                for diff in diffs.into_iter().map(|d| {
                    d.map(|m| {
                        (
                            m.unique_id().0.clone(),
                            m.convert(avatar_cache, thumbnail_cache),
                        )
                    })
                }) {
                    match diff {
                        VectorDiff::Append { values } => {
                            for (key, value) in values {
                                messages.insert(key, value);
                            }
                        }
                        VectorDiff::Clear => {
                            messages.clear();
                        }
                        VectorDiff::Insert {
                            index,
                            value: (key, value),
                        } => {
                            messages.shift_insert(index, key, value);
                        }
                        VectorDiff::PopBack => {
                            messages.pop();
                        }
                        VectorDiff::PopFront => {
                            messages.shift_remove_index(0);
                        }
                        VectorDiff::PushBack {
                            value: (key, value),
                        } => {
                            messages.insert(key, value);
                        }
                        VectorDiff::PushFront {
                            value: (key, value),
                        } => {
                            messages.shift_insert(0, key, value);
                        }
                        VectorDiff::Remove { index } => {
                            messages.shift_remove_index(index);
                        }
                        VectorDiff::Reset { values } => {
                            messages.clear();
                            for (key, value) in values {
                                messages.insert(key, value);
                            }
                        }
                        VectorDiff::Set {
                            index,
                            value: (key, value),
                        } => {
                            messages.shift_remove_index(index);
                            messages.shift_insert(index, key, value);
                        }
                        VectorDiff::Truncate { length } => {
                            messages.truncate(length);
                        }
                    }
                }
                self.messages_version += 1;
            }
            TimelineMessage::SetReplying(event_id) => return TimelineAction::SetReplying(event_id),
            TimelineMessage::None => {}
        };

        TimelineAction::None
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, TimelineMessage> {
        w::container(
            w::scrollable(
                w::keyed_column(self.messages.iter().map(|(id, item)| {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    id.hash(&mut hasher);
                    let key = hasher.finish();

                    let id = id.clone();
                    let item = item.clone();
                    let element = w::lazy(item.clone(), move |item| {
                        let id = id.clone();
                        item.view(theme, structure)
                            .map(move |msg| TimelineMessage::Item {
                                id: id.clone(),
                                message: msg,
                            })
                    });
                    (key, element.into())
                }))
                .spacing(structure.small_gap)
                .width(Fill),
            )
            .anchor_bottom()
            .width(Fill),
        )
        .width(Fill)
        .align_bottom(Fill)
        .into()
    }
}
