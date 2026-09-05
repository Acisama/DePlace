use std::collections::HashMap;

use deplace_core::PaginationDirection;
use macros::iced_cache;
use matrix_sdk_ui::{Timeline, eyeball_im::VectorDiff, timeline::TimelineItem as UiTimelineItem};
use messages::{TimelineItem, TimelineItemAction, TimelineItemMessage};
use sweeten::widget::list;

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
        initial: Arc<Vec<Arc<TimelineItem>>>,
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

    content: list::Content<Arc<TimelineItem>>,
    id_index: Arc<HashMap<String, usize>>,
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

            content: list::Content::default(),
            id_index: Arc::new(HashMap::new()),
            messages_version: 0,
        }
    }

    pub fn load_media(&mut self, media: &MediaLoaded) {
        let mut changed = false;
        for index in 0..self.content.len() {
            if let Some(item) = self.content.get_mut(index) {
                changed |= Arc::make_mut(item).load_media(media);
            }
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
                let Some(&index) = self.id_index.get(&id) else {
                    tracing::warn!("No item found for id {}", id);
                    return None;
                };
                let Some(item) = self.content.get_mut(index) else {
                    tracing::warn!("No item found for id {}", id);
                    return None;
                };
                return match Arc::make_mut(item).update(message) {
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
                self.id_index = Arc::new(
                    initial
                        .iter()
                        .enumerate()
                        .map(|(index, item)| (item.id.clone(), index))
                        .collect(),
                );
                self.content = list::Content::with_items((*initial).clone());
                self.messages_version += 1;
            }
            TimelineMessage::Diffs(diffs) => {
                tracing::debug!(
                    "ChatTimeline for room {} applying {} diff(s), {} messages before",
                    self.room_id,
                    diffs.len(),
                    self.content.len()
                );

                let avatar_cache = self.state.avatar_cache();
                let thumbnail_cache = self.state.thumbnail_cache();
                let video_cache = self.state.video_cache();

                for diff in diffs.into_iter().map(|d| {
                    d.map(|m| Arc::new(m.convert(avatar_cache, thumbnail_cache, video_cache)))
                }) {
                    match diff {
                        VectorDiff::Append { values } => {
                            for value in values {
                                self.content.push(value);
                            }
                        }
                        // A wholesale reset -- nothing incremental to
                        // preserve, so a fresh Content is both correct and
                        // cheaper than removing every item one by one.
                        VectorDiff::Clear => self.content = list::Content::new(),
                        VectorDiff::Insert { index, value } => {
                            self.content.insert(index, value);
                        }
                        VectorDiff::PopBack => {
                            if !self.content.is_empty() {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                        VectorDiff::PopFront => {
                            if !self.content.is_empty() {
                                self.content.remove(0);
                            }
                        }
                        VectorDiff::PushBack { value } => self.content.push(value),
                        VectorDiff::PushFront { value } => self.content.insert(0, value),
                        VectorDiff::Remove { index } => {
                            self.content.remove(index);
                        }
                        VectorDiff::Reset { values } => {
                            self.content = list::Content::with_items(values.into_iter().collect());
                        }
                        VectorDiff::Set { index, value } => {
                            if let Some(slot) = self.content.get_mut(index) {
                                *slot = value;
                            }
                        }
                        VectorDiff::Truncate { length } => {
                            while self.content.len() > length {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                    }
                }

                self.id_index = Arc::new(
                    (0..self.content.len())
                        .filter_map(|index| {
                            self.content.get(index).map(|item| (item.id.clone(), index))
                        })
                        .collect(),
                );
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

                match direction {
                    PaginationDirection::Forward => self.loading_bottom = true,
                    PaginationDirection::Backward => self.loading_top = true,
                };

                tracing::trace!("Scrolling to edge: direction={:?}", direction);

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
        let reached_top = self.reached_top;
        let reached_bottom = self.reached_bottom;
        let loading_top = self.loading_top;
        let loading_bottom = self.loading_bottom;

        w::container(
            w::scrollable(
                w::container(list(self.content.clone(), move |_index, item| {
                    let id = item.id.clone();
                    w::lazy(item.clone(), move |item| {
                        let id = id.clone();
                        item.view(theme, structure)
                            .map(move |msg| TimelineMessage::Item {
                                id: id.clone(),
                                message: msg,
                            })
                    })
                    .into()
                }))
                .padding(10)
                .width(Fill),
            )
            .width(Fill)
            .anchor_bottom()
            .on_scroll(move |viewport| {
                // With `anchor_bottom`, `absolute_offset` is distance
                // scrolled *up from the bottom*: 0 at the bottom, growing
                // towards `max_offset` at the top.
                let max_offset =
                    (viewport.content_bounds().height - viewport.bounds().height).max(0.0);
                let offset = viewport.absolute_offset().y;

                if !reached_top
                    && !loading_top
                    && max_offset - offset <= structure.chat.icon_size * 6.0
                {
                    return TimelineMessage::Scroll {
                        direction: PaginationDirection::Backward,
                    };
                }
                if !reached_bottom && !loading_bottom && offset <= structure.chat.icon_size * 6.0 {
                    return TimelineMessage::Scroll {
                        direction: PaginationDirection::Forward,
                    };
                }
                TimelineMessage::None
            }),
        )
        .width(Fill)
        .align_bottom(Fill)
        .into()
    }
}
