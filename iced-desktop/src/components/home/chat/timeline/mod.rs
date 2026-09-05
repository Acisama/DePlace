use std::hash::Hasher;

use deplace_core::PaginationDirection;
use macros::iced_cache;
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
    Loaded {
        timeline: Arc<Timeline>,
        initial: Arc<IndexMap<String, TimelineItem>>,
    },
    Diffs(Vec<VectorDiff<Arc<UiTimelineItem>>>),
    SetReplying(OwnedEventId),
    None,
    Scroll {
        direction: PaginationDirection,
    },
}

pub enum TimelineAction {
    NeedsMedia(NeedsMedia),
    SetReplying(OwnedEventId),
    Run(Task<()>),
    Scroll {
        direction: PaginationDirection,
        task: Task<bool>,
    },
}

#[iced_cache(Clone)]
pub struct ChatTimeline {
    state: AppState,
    timeline: Option<Arc<Timeline>>,

    #[hash]
    room_id: OwnedRoomId,

    reached_top: bool,
    reached_bottom: bool,
    loading_top: bool,
    loading_bottom: bool,

    messages: Arc<IndexMap<String, TimelineItem>>,
    #[hash]
    messages_version: u64,
}

impl ChatTimeline {
    pub fn new(room: &Room, state: &AppState) -> Self {
        Self {
            state: state.clone(),
            timeline: None,
            room_id: room.room_id().to_owned(),

            reached_top: false,
            reached_bottom: false,
            loading_top: false,
            loading_bottom: false,

            messages: Arc::new(IndexMap::new()),
            messages_version: 0,
        }
    }

    pub fn load_media(&mut self, media: &MediaLoaded) {
        let mut changed = false;
        for item in Arc::make_mut(&mut self.messages).values_mut() {
            changed &= item.load_media(media);
        }

        if changed {
            self.messages_version += 1;
        }
    }

    pub fn set_scroll_finished(&mut self, direction: PaginationDirection, finished: bool) {
        match direction {
            PaginationDirection::Forward => {
                self.reached_bottom = finished;
                self.loading_bottom = false;
            }
            PaginationDirection::Backward => {
                self.reached_top = finished;
                self.loading_top = false;
            }
        }
    }
}

impl IcedWidget<TimelineMessage, TimelineAction> for ChatTimeline {
    fn update(&mut self, message: TimelineMessage) -> Option<TimelineAction> {
        match message {
            TimelineMessage::Item { id, message } => {
                let Some(item) = Arc::make_mut(&mut self.messages).get_mut(&id) else {
                    tracing::warn!("No item found for id {}", id);
                    return None;
                };
                return match item.update(message) {
                    Some(TimelineItemAction::Update) => {
                        self.messages_version += 1;
                        None
                    }
                    Some(TimelineItemAction::NeedsMedia(needs_media)) => {
                        Some(TimelineAction::NeedsMedia(needs_media))
                    }
                    _ => None,
                };
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
                let avatar_cache = self.state.avatar_cache();
                let thumbnail_cache = self.state.thumbnail_cache();
                let video_cache = self.state.video_cache();

                for diff in diffs.into_iter().map(|d| {
                    d.map(|m| {
                        (
                            m.unique_id().0.clone(),
                            m.convert(avatar_cache, thumbnail_cache, video_cache),
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
            TimelineMessage::SetReplying(event_id) => {
                return Some(TimelineAction::SetReplying(event_id));
            }
            TimelineMessage::Scroll { direction } => {
                let already_loading = match direction {
                    PaginationDirection::Forward => self.loading_bottom,
                    PaginationDirection::Backward => self.loading_top,
                };

                if already_loading {
                    return None;
                }

                let timeline = self.timeline.clone()?;
                return Some(TimelineAction::Scroll {
                    direction,
                    task: Task::future(async move {
                        match direction {
                            PaginationDirection::Forward => timeline.paginate_forwards(30).await,
                            PaginationDirection::Backward => timeline.paginate_backwards(30).await,
                        }
                        .map_err(|e| {
                            tracing::error!("Failed to paginate: {}", e);
                        })
                        // Treat failure to paginate like hitting the end
                        .unwrap_or(true)
                    }),
                });
            }
            TimelineMessage::None => {}
        };

        None
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, TimelineMessage> {
        w::container(
            w::scrollable(
                w::column![
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
                    Space::new().height(structure.gap * 3.0)
                ]
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
