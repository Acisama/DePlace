use std::collections::{BTreeSet, HashMap};

use deplace_core::PaginationDirection;
use macros::iced_cache;
use matrix_sdk::media::UniqueKey;
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

    avatar_cache: AvatarCache,
    thumbnail_cache: ThumbnailCache,
    video_cache: VideoCache,

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

            avatar_cache: state.avatar_cache().clone(),
            thumbnail_cache: state.thumbnail_cache().clone(),
            video_cache: state.video_cache().clone(),

            avatar_states_for_hash: BTreeSet::new(),
            thumbnail_states_for_hash: BTreeSet::new(),
            video_states_for_hash: BTreeSet::new(),

            reached_top: false,
            reached_bottom: false,
            loading_top: false,
            loading_bottom: false,

            content: list::Content::default(),
            id_index: Arc::new(HashMap::new()),
            messages_version: 0,
        }
    }

    /// The virtualized `List` only re-examines a row's hash when `get_mut`
    /// queues a `Change::Updated` for it; this method pokes every row to trigger
    /// re-examination when the media is actually loaded.
    pub fn touch_media(&mut self, media: &MediaLoaded) {
        let relevant = match media {
            MediaLoaded::Avatar { uri } => self.avatar_states_for_hash.contains(uri),
            MediaLoaded::Thumbnail { key } => self.thumbnail_states_for_hash.contains(key),
            MediaLoaded::Video { key } => self.video_states_for_hash.contains(key),
        };

        if !relevant {
            return;
        }

        for index in 0..self.content.len() {
            self.content.get_mut(index);
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

    fn pagination_task(
        &mut self,
        direction: PaginationDirection,
        amount: u16,
    ) -> Option<TimelineAction> {
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
        Some(TimelineAction::Scroll {
            direction,
            task: Task::future(async move {
                match direction {
                    PaginationDirection::Forward => timeline.paginate_forwards(amount).await,
                    PaginationDirection::Backward => timeline.paginate_backwards(amount).await,
                }
                .map_err(|e| {
                    tracing::error!("Failed to paginate: {}", e);
                })
                // Treat failure to paginate like hitting the end
                .unwrap_or(true)
            }),
        })
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
                        match &needs_media {
                            NeedsMedia::Avatar { uri } => {
                                self.avatar_states_for_hash.insert(uri.clone());
                            }
                            NeedsMedia::Thumbnail { key, .. } => {
                                self.thumbnail_states_for_hash.insert(key.clone());
                            }
                            NeedsMedia::Video { source, .. } => {
                                self.video_states_for_hash.insert(source.unique_key());
                            }
                        }
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

                if initial.len() < 50 {
                    return self
                        .pagination_task(PaginationDirection::Forward, 50 - initial.len() as u16);
                }
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
                return self.pagination_task(direction, 30);
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
                .padding(padding::bottom(structure.gap * 3.0))
                .width(Fill),
            )
            .width(Fill)
            .anchor_bottom()
            .on_scroll(move |viewport| {
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
