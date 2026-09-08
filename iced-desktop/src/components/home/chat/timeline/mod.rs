use std::collections::{BTreeSet, HashMap};

use deplace_core::PaginationDirection;
use macros::iced_cache;
use matrix_sdk::{
    media::UniqueKey,
    ruma::{events::room::power_levels::RoomPowerLevels, room_version_rules::AuthorizationRules},
};
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
        power_levels: Arc<RoomPowerLevels>,
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
    SetIsReplyingTo(OwnedEventId),
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
    // TODO: Add logic to update this
    power_levels: Arc<RoomPowerLevels>,

    #[hash]
    room_id: OwnedRoomId,
    own_user_id: OwnedUserId,

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
            power_levels: Arc::new(RoomPowerLevels::new(
                matrix_sdk::ruma::events::room::power_levels::RoomPowerLevelsSource::None,
                &AuthorizationRules::V12,
                [],
            )),

            room_id: room.room_id().to_owned(),
            own_user_id: room.own_user_id().to_owned(),

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

    // TODO: Actually use this
    pub fn recalculate_with_power_levels(&mut self, own_user_id: &UserId) {
        for index in 0..self.content.len() {
            if let Some(item) = self.content.get_mut(index) {
                Arc::make_mut(item).recalculate_with_power_levels(&self.power_levels, own_user_id);
            }
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

    /// Recomputes `connects_previous`/`previous_is_text_message` for a
    /// single index, applying the update only where it actually changed.
    /// Both only depend on the *previous* neighbor.
    fn refresh_previous_linkage(&mut self, index: usize) {
        let previous = index
            .checked_sub(1)
            .and_then(|p| self.content.get(p))
            .cloned();

        let Some(current) = self.content.get(index) else {
            return;
        };

        let connection = current.compute_connection(previous.as_deref());
        let prev_is_text = current.compute_prev_is_event(previous.as_deref());

        if connection.is_none() && prev_is_text.is_none() {
            return;
        }

        if let Some(item) = self.content.get_mut(index) {
            let item = Arc::make_mut(item);
            if let Some(connects_before) = connection {
                item.set_connection(connects_before);
            }
            if let Some(prev_is_text) = prev_is_text {
                item.set_previous_is_event(prev_is_text);
            }
        }
    }

    /// After a structural change at `index` (insert/remove/push/pop),
    /// refreshes `index` and the item right after it -- since the linkage
    /// only ever looks backward, those are the only two items whose
    /// `previous` neighbor could possibly have changed.
    fn refresh_previous_linkage_near(&mut self, index: usize) {
        self.refresh_previous_linkage(index);
        self.refresh_previous_linkage(index + 1);
    }

    /// Recomputes the previous-neighbor linkage for every item -- used
    /// after bulk operations (initial load, reset) where structure changed
    /// too broadly to target specific indices.
    fn refresh_all_previous_linkage(&mut self) {
        for index in 0..self.content.len() {
            self.refresh_previous_linkage(index);
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
                return if let Some(action) = Arc::make_mut(item).update(message) {
                    match action {
                        TimelineItemAction::Update => {
                            self.messages_version += 1;
                            None
                        }
                        TimelineItemAction::NeedsMedia(needs_media) => {
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
                        TimelineItemAction::SetIsReplyingTo(event_id) => {
                            Some(TimelineAction::SetIsReplyingTo(event_id.clone()))
                        }
                    }
                } else {
                    None
                };
            }
            TimelineMessage::Loaded {
                timeline,
                initial,
                power_levels,
            } => {
                tracing::debug!(
                    "ChatTimeline for room {} loaded with {} initial items",
                    self.room_id,
                    initial.len()
                );
                self.timeline = Some(timeline);
                self.power_levels = power_levels;

                self.id_index = Arc::new(
                    initial
                        .iter()
                        .enumerate()
                        .map(|(index, item)| (item.id.clone(), index))
                        .collect(),
                );
                self.content = list::Content::with_items((*initial).clone());
                self.refresh_all_previous_linkage();
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

                let avatar_cache = self.state.avatar_cache().clone();
                let thumbnail_cache = self.state.thumbnail_cache().clone();
                let video_cache = self.state.video_cache().clone();
                let room_id = self.room_id.clone();
                let power_levels = self.power_levels.clone();
                let own_user_id = self.own_user_id.clone();

                for diff in diffs.into_iter().map(|d| {
                    d.map(|m| {
                        Arc::new(m.convert(
                            &avatar_cache,
                            &thumbnail_cache,
                            &video_cache,
                            room_id.clone(),
                            &power_levels,
                            &own_user_id,
                        ))
                    })
                }) {
                    match diff {
                        VectorDiff::Append { values } => {
                            for value in values {
                                self.content.push(value);
                                // Last item only: appending never has
                                // anything after it whose `previous` could
                                // change.
                                self.refresh_previous_linkage(self.content.len() - 1);
                            }
                        }
                        VectorDiff::Clear => self.content = list::Content::new(),
                        VectorDiff::Insert { index, value } => {
                            self.content.insert(index, value);
                            self.refresh_previous_linkage_near(index);
                        }
                        // Removing from the end can't change any remaining
                        // item's `previous` neighbor.
                        VectorDiff::PopBack => {
                            if !self.content.is_empty() {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                        VectorDiff::PopFront => {
                            if !self.content.is_empty() {
                                self.content.remove(0);
                                self.refresh_previous_linkage(0);
                            }
                        }
                        VectorDiff::PushBack { value } => {
                            self.content.push(value);
                            self.refresh_previous_linkage(self.content.len() - 1);
                        }
                        VectorDiff::PushFront { value } => {
                            self.content.insert(0, value);
                            self.refresh_previous_linkage_near(0);
                        }
                        VectorDiff::Remove { index } => {
                            self.content.remove(index);
                            self.refresh_previous_linkage(index);
                        }
                        VectorDiff::Reset { values } => {
                            self.content = list::Content::with_items(values.into_iter().collect());
                            self.refresh_all_previous_linkage();
                        }
                        // Matrix timeline updates (edits, reactions,
                        // redactions-in-place) never change an item's
                        // sender, timestamp, or whether it's a message at
                        // all -- the only things the linkage depends on --
                        // and don't shift any other item's position either,
                        // so nothing here can ever need recomputing.
                        VectorDiff::Set { index, value } => {
                            if let Some(slot) = self.content.get_mut(index) {
                                *slot = value;
                            }
                        }
                        // Truncating only removes from the end, which can't
                        // change any remaining item's `previous` neighbor.
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
                return Some(TimelineAction::SetIsReplyingTo(event_id));
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
            themed_scrollable(
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
                theme,
                structure,
            )
            .spacing(structure.small_gap)
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
