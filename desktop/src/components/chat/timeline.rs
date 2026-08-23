use std::{ops::Range, sync::Arc};

use dashmap::DashMap;
use deplace_core::{
    matrix_api::messages::RoomSendingExt,
    state::{AppState, MembershipMap},
};
use futures_util::StreamExt;
use gpui::{
    App, AppContext, Context, Empty, Entity, EventEmitter, FocusHandle, Focusable, FollowMode,
    InteractiveElement, IntoElement, ListAlignment, ListScrollEvent, ListState, ParentElement,
    Render, SharedString, Styled, Task, WeakEntity, Window, actions, list, px,
};
use gpui_component::{
    StyledExt,
    input::{self, InputState},
};
use macros::tailwind_div;
use matrix_sdk::{
    Room,
    ruma::{OwnedEventId, OwnedUserId, UserId},
};
use matrix_sdk_ui::{
    Timeline,
    eyeball_im::{Vector, VectorDiff},
    timeline::{
        DateDividerMode, TimelineBuilder, TimelineEventItemId, TimelineFocus, TimelineItem,
        TimelineReadReceiptTracking,
    },
};
use serde::Deserialize;
use tokio::{
    runtime::Runtime,
    sync::watch::Receiver,
    task::{AbortHandle, JoinHandle},
};
use tracing::error;

use crate::{
    cache::{AvatarCache, ThumbnailCache},
    components::chat::{
        input::SendEvent,
        message::{CachedTimelineItem, CachedTimelineItemKind, cached_from_timeline_item},
    },
    things::DeplaceThings,
    watch_bridge::notify_on_change,
};

/// Payload enum for communication
enum TimelineMessagePayload {
    Initialized {
        timeline: Box<Timeline>,
        initial_messages: Vector<Arc<TimelineItem>>,
    },
    Diffs(Vec<VectorDiff<Arc<TimelineItem>>>),
}

/// Enum to feed into [`check_pagination`]
///
/// Can be either constructed with the range of visible
/// items or an index of a focused item
pub enum ScrollOffset {
    VisibleRange { range: Range<usize> },
    FocusedIndex { index: usize },
}

#[derive(Default, Clone, Copy)]
pub struct PaginationState {
    pub reached_start: bool,
    pub reached_end: bool,
}

#[derive(Clone, Copy)]
pub enum ScrollDirection {
    Up,
    Down,
}

impl std::fmt::Display for ScrollDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScrollDirection::Up => write!(f, "up"),
            ScrollDirection::Down => write!(f, "down"),
        }
    }
}

pub struct TimelineView {
    pub messages: Arc<Vec<CachedTimelineItem>>,

    /// Which message you are focusing and editing
    ///
    /// 1. `usize` is the index of the focused message
    /// among the loaded messages
    /// 2. `bool` is whether the focused message is being
    /// edited
    pub focused_message: Option<(usize, bool)>,
    editing_message: Entity<InputState>,
    active_room: Room,
    timeline: Option<Arc<Timeline>>,

    avatar_cache: AvatarCache,
    image_cache: ThumbnailCache,

    state: AppState,
    pagination_state: PaginationState,

    reactions_in_flight: Arc<DashMap<(Arc<OwnedEventId>, SharedString), ()>>,

    membership_map: Receiver<MembershipMap>,
    tokio_rt: Arc<Runtime>,
    current_scroll_up: Option<AbortHandle>,
    current_scroll_down: Option<AbortHandle>,
    user_id: OwnedUserId,
    _updates_task: (Task<()>, JoinHandle<()>),
    pub list_state: ListState,
    focus_handle: FocusHandle,
}

impl EventEmitter<SendEvent> for TimelineView {}

impl TimelineView {
    /// Creates a new `TimelineView` and initializes its message stream for the active room.
    ///
    /// # Cross-Runtime Bug Reference
    ///
    ///
    /// * Polling Tokio primitives or using `tokio::sync::mpsc` receivers inside
    ///   GPUI's `cx.spawn` can orphan task wakers.  This causes updates to freeze
    ///   silently without errors, panics, or task cancellation.
    /// * The Fix: The Matrix stream collection loop runs entirely within the dedicated
    ///   Tokio runtime (`tokio_rt`), utilizing a runtime-agnostic `async_channel` to safely
    ///   bridge updates across to GPUI's UI thread via `weak_self.update`.
    pub fn new(
        state: &AppState,
        cx: &mut Context<Self>,
        window: &mut Window,
        tokio_rt: Arc<Runtime>,
        avatar_cache: AvatarCache,
        image_cache: ThumbnailCache,
        active_room: Room,
    ) -> Self {
        let list_state = ListState::new(0, ListAlignment::Bottom, px(500.));
        list_state.set_follow_mode(FollowMode::Tail);
        list_state.set_scroll_handler(cx.listener(|this, event: &ListScrollEvent, _window, cx| {
            this.check_pagination(
                ScrollOffset::VisibleRange {
                    range: event.visible_range.clone(),
                },
                cx,
            );
        }));

        let membership_map = state.membership_map();
        let settings = state.settings();

        notify_on_change(membership_map.clone(), cx);
        notify_on_change(avatar_cache.subscribe(), cx);
        notify_on_change(image_cache.subscribe(), cx);

        notify_on_change(settings.watch_data_size_unit(), cx);
        notify_on_change(settings.watch_system_messages_to_show(), cx);

        let room_id = active_room.room_id().to_owned();

        tracing::debug!("Loading timeline for room {}", room_id);

        // Use async_channel
        let (tx, rx) = async_channel::bounded(100);
        let room_id_tokio = room_id.clone();

        let timeline_builder = TimelineBuilder::new(&active_room)
            .with_date_divider_mode(DateDividerMode::Daily)
            .with_focus(TimelineFocus::Live {
                hide_threaded_events: false,
            })
            .track_read_marker_and_receipts(TimelineReadReceiptTracking::AllEvents)
            .add_failed_to_parse(true);

        // matrix-sdk stream loop completely inside tokio
        let tokio_task = tokio_rt.spawn(async move {
            let timeline = match timeline_builder.build().await {
                Ok(timeline) => timeline,
                Err(e) => {
                    tracing::error!("Timeline build failed: {e:?}");
                    return;
                }
            };

            let (initial_messages, mut update_stream) = timeline.subscribe().await;

            tracing::debug!(
                "Loaded timeline for room {}: {} messages",
                room_id_tokio,
                initial_messages.len()
            );

            // Send initial load to GPUI
            if let Err(e) = tx
                .send(TimelineMessagePayload::Initialized {
                    timeline: Box::new(timeline),
                    initial_messages,
                })
                .await
            {
                error!("{}", e);
                return;
            }

            // poll the Matrix stream and forward via async_channel
            while let Some(diffs) = update_stream.next().await {
                if let Err(e) = tx.send(TimelineMessagePayload::Diffs(diffs)).await {
                    error!("{}", e);
                    break;
                }
            }
            tracing::info!("Timeline update stream ended for room {}", room_id_tokio);
        });

        let weak_self = cx.entity().downgrade();
        let room_id_gpui = room_id.clone();

        // consume items cleanly on gpui's foreground thread pool completely distinct
        // from tokios runtime via the runtime agnostic async_channel
        let updates_task = cx.spawn(async move |_this, cx| {
            while let Ok(payload) = rx.recv().await {
                let updated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Err(e) = weak_self.update(cx, |view, cx| {
                        match payload {
                            TimelineMessagePayload::Initialized {
                                timeline,
                                initial_messages,
                            } => {
                                let mut cached_messages: Vec<CachedTimelineItem> = initial_messages
                                    .iter()
                                    .map(|item| cached_from_timeline_item(item, &view.user_id))
                                    .collect();

                                for i in 0..cached_messages.len() {
                                    let Some((current_item, rest)) =
                                        cached_messages[i..].split_first_mut()
                                    else {
                                        continue;
                                    };

                                    current_item.recompute_datedivider_types(rest);
                                }

                                view.messages = Arc::new(cached_messages);
                                view.timeline = Some(Arc::new(*timeline));

                                let len = view.messages.len();
                                recompute_grouping_range(&mut view.messages, 0..len);
                                view.list_state.splice(0..0, view.messages.len());
                                view.list_state.set_follow_mode(FollowMode::Tail);
                            }
                            TimelineMessagePayload::Diffs(diffs) => {
                                for diff in diffs {
                                    apply_diff(
                                        &mut view.messages,
                                        &mut view.focused_message,
                                        &view.list_state,
                                        diff,
                                        &view.user_id,
                                    );
                                }
                            }
                        }
                        cx.notify();
                    }) {
                        tracing::error!("Failed to update timeline view: {:?}", e);
                    };
                }));

                match updated {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::error!(
                            "Panic while applying timeline payload for room {}: {:?}",
                            room_id_gpui,
                            e
                        );
                    }
                }
            }
        });

        Self {
            user_id: state.user_device().user_id.clone(),
            messages: Arc::new(Vec::new()),
            state: state.clone(),
            focused_message: None,
            editing_message: cx.new(|cx| InputState::new(window, cx)),
            avatar_cache,
            image_cache,
            membership_map,
            tokio_rt,
            timeline: None,
            active_room,
            current_scroll_up: None,
            current_scroll_down: None,
            _updates_task: (updates_task, tokio_task),
            list_state,
            focus_handle: cx.focus_handle(),

            pagination_state: PaginationState::default(),

            reactions_in_flight: Arc::new(DashMap::new()),
        }
    }

    fn scroll(&mut self, cx: &mut Context<Self>, direction: ScrollDirection) {
        let current_scroll = match direction {
            ScrollDirection::Up => {
                if self.current_scroll_up.is_some() || self.pagination_state.reached_end {
                    return;
                }
                &mut self.current_scroll_up
            }
            ScrollDirection::Down => {
                if self.current_scroll_down.is_some() || self.pagination_state.reached_start {
                    return;
                }
                &mut self.current_scroll_down
            }
        };

        let Some(timeline) = self.timeline.clone() else {
            return;
        };

        let task = self.tokio_rt.spawn(async move {
            match direction {
                ScrollDirection::Up => timeline.paginate_backwards(30).await,
                ScrollDirection::Down => timeline.paginate_forwards(30).await,
            }
        });
        *current_scroll = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            let result: Result<
                Result<bool, matrix_sdk_ui::timeline::Error>,
                tokio::task::JoinError,
            > = task.await;
            if let Err(e) = this.update(cx, |view, _| {
                let current_scroll = match direction {
                    ScrollDirection::Up => &mut view.current_scroll_up,
                    ScrollDirection::Down => &mut view.current_scroll_down,
                };
                *current_scroll = None;
            }) {
                tracing::debug!(
                    "Failed to reset scroll guard for {}, entity is gone: {:?}",
                    direction,
                    e
                );
            }
            match result {
                Ok(Ok(direction_end)) => {
                    if let Err(e) = this.update(cx, |view, _| match direction {
                        ScrollDirection::Up => view.pagination_state.reached_end = direction_end,
                        ScrollDirection::Down => {
                            view.pagination_state.reached_start = direction_end
                        }
                    }) {
                        tracing::error!("Failed to update pagination state: {}", e);
                    }
                }
                Ok(Err(e)) => {
                    tracing::error!("Failed to scroll timeline: {}", e);
                    if let Err(e) = this.update(cx, |view, _| match direction {
                        ScrollDirection::Up => view.pagination_state.reached_end = false,
                        ScrollDirection::Down => view.pagination_state.reached_start = false,
                    }) {
                        tracing::error!("Failed to update pagination state: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Join error: {}", e);
                }
            }
        })
        .detach();
    }

    pub fn toggle_reaction(&self) -> impl Fn(Arc<OwnedEventId>, SharedString) + Clone + 'static {
        let tokio_rt = self.tokio_rt.clone();
        let in_flight = self.reactions_in_flight.clone();
        let timeline = self.timeline.clone();

        move |event_id: Arc<OwnedEventId>, reaction: SharedString| {
            let Some(timeline) = timeline.clone() else {
                return;
            };

            let key = (event_id.clone(), reaction.clone());
            if in_flight.insert(key.clone(), ()).is_some() {
                return;
            }

            let in_flight = in_flight.clone();
            tokio_rt.spawn(async move {
                if let Err(e) = timeline
                    .toggle_reaction(
                        &TimelineEventItemId::EventId((*event_id).clone()),
                        &reaction,
                    )
                    .await
                {
                    tracing::error!("Failed to toggle reaction: {}", e);
                }
                in_flight.remove(&key);
            });
        }
    }

    pub fn check_pagination(&mut self, scroll_offset: ScrollOffset, cx: &mut Context<Self>) {
        const EDGE_THRESHOLD: usize = 10;

        let length = self.messages.len();

        let upper_check: usize;
        let lower_check: usize;
        match scroll_offset {
            ScrollOffset::VisibleRange { range } => {
                upper_check = range.start;
                lower_check = length.saturating_sub(range.end);
            }
            ScrollOffset::FocusedIndex { index } => {
                upper_check = index;
                lower_check = length.saturating_sub(index);
            }
        }

        if upper_check < EDGE_THRESHOLD {
            self.scroll(cx, ScrollDirection::Up);
        }
        if lower_check < EDGE_THRESHOLD {
            self.scroll(cx, ScrollDirection::Down);
        }
    }

    /// Mark the message at the index for editing
    ///
    /// This produces `None` if editing could not be started
    ///
    /// Also sets the value of the editing inputstate to the
    /// text of the message and  subscribes to the Enter or Escape actions
    /// of the input in order to easily terminate.
    pub fn try_edit_message(
        &mut self,
        window: &mut Window,
        cx: &mut App,
        index: usize,
    ) -> Option<()> {
        let Some(message) = self.messages.get(index) else {
            return None;
        };
        if !message.is_sent_by(&self.user_id) {
            return None;
        }
        let Some(text) = message.retrieve_text() else {
            return None;
        };
        if let Some((_, editing)) = &mut self.focused_message {
            *editing = true
        };
        self.editing_message.update(cx, |this, cx| {
            this.set_value(text, window, cx);
            this.focus(window, cx);
        });

        Some(())
    }

    /// Resets the editing flag and restores focus to chat
    fn cancel_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        tracing::debug!("Canceling editing message");
        if let Some((_, editing)) = &mut self.focused_message {
            if !*editing {
                tracing::warn!(
                    "Received action to cancel editing message without currently editing message"
                );
            }
            *editing = false;
        }
        cx.focus_self(window);
        cx.notify();
    }
}

fn recompute_show_header_at(messages: &mut Arc<Vec<CachedTimelineItem>>, index: usize) {
    if index >= messages.len() {
        return;
    }
    let prev = index.checked_sub(1).and_then(|i| messages.get(i)).cloned();
    Arc::make_mut(messages)[index].recompute_show_header(prev.as_ref());
}

fn recompute_pad_bottom_at(messages: &mut Arc<Vec<CachedTimelineItem>>, index: usize) {
    if index >= messages.len() {
        return;
    }
    let next = messages.get(index + 1).cloned();
    Arc::make_mut(messages)[index].recompute_pad_bottom(next.as_ref());
}

fn recompute_grouping_range(messages: &mut Arc<Vec<CachedTimelineItem>>, range: Range<usize>) {
    for index in range {
        recompute_show_header_at(messages, index);
        recompute_pad_bottom_at(messages, index);
    }
}

/// Recomputes the date divider(s) affected by a change at `index`: the item at
/// `index` itself (if it's a divider) and the nearest divider preceding it,
/// since inserting/removing/changing an item can change what a divider's
/// forward-looking scan sees.
fn recompute_datedivider_near(messages: &mut Arc<Vec<CachedTimelineItem>>, index: usize) {
    let messages = Arc::make_mut(messages);

    if index < messages.len()
        && matches!(
            messages[index].kind,
            CachedTimelineItemKind::DateDivider { .. }
        )
        && let Some((current_item, rest)) = messages[index..].split_first_mut()
    {
        current_item.recompute_datedivider_types(rest);
    }

    if let Some(p) = messages[..index.min(messages.len())]
        .iter()
        .rposition(|item| matches!(item.kind, CachedTimelineItemKind::DateDivider { .. }))
        && let Some((current_item, rest)) = messages[p..].split_first_mut()
    {
        current_item.recompute_datedivider_types(rest);
    }
}

fn apply_diff(
    messages: &mut Arc<Vec<CachedTimelineItem>>,
    focused_message: &mut Option<(usize, bool)>,
    list_state: &ListState,
    diff: VectorDiff<Arc<TimelineItem>>,
    own_id: &UserId,
) {
    match diff {
        VectorDiff::Append { values } => {
            let start = messages.len();

            Arc::make_mut(messages).extend(
                values
                    .iter()
                    .map(|item| cached_from_timeline_item(item, own_id)),
            );
            list_state.splice(start..start, messages.len() - start);

            for i in 0..messages.len() {
                let Some((current_item, rest)) = Arc::make_mut(messages)[i..].split_first_mut()
                else {
                    continue;
                };

                current_item.recompute_datedivider_types(rest);
            }

            if start > 0 {
                recompute_pad_bottom_at(messages, start - 1);
            }
            recompute_grouping_range(messages, start..messages.len());
        }
        VectorDiff::Clear => {
            list_state.splice(0..messages.len(), 0);
            Arc::make_mut(messages).clear();
            *focused_message = None; // Fix: Reset focus on clear
        }
        VectorDiff::PushFront { value } => {
            Arc::make_mut(messages).insert(0, cached_from_timeline_item(&value, own_id));
            list_state.splice(0..0, 1);

            if let Some((current_item, rest)) = Arc::make_mut(messages).split_first_mut() {
                current_item.recompute_datedivider_types(rest);
            }

            if let Some((focused, _)) = focused_message {
                *focused += 1;
            }

            recompute_show_header_at(messages, 0);
            recompute_pad_bottom_at(messages, 0);
            recompute_show_header_at(messages, 1);
        }
        VectorDiff::PushBack { value } => {
            Arc::make_mut(messages).push(cached_from_timeline_item(&value, own_id));
            list_state.splice(messages.len() - 1..messages.len() - 1, 1);

            let new_index = messages.len() - 1;
            recompute_show_header_at(messages, new_index);
            recompute_pad_bottom_at(messages, new_index);
            if new_index > 0 {
                recompute_pad_bottom_at(messages, new_index - 1);
            }
        }
        VectorDiff::PopFront => {
            if !messages.is_empty() {
                Arc::make_mut(messages).remove(0);
                list_state.splice(0..1, 0);

                if let Some((focused, _)) = focused_message {
                    if *focused == 0 {
                        *focused_message = None;
                    } else {
                        *focused -= 1;
                    }
                }

                recompute_show_header_at(messages, 0);
            }
        }
        VectorDiff::PopBack => {
            if Arc::make_mut(messages).pop().is_some() {
                let len = messages.len();
                list_state.splice(len..len + 1, 0);

                if let Some((focused, _)) = focused_message {
                    if *focused >= len {
                        *focused_message = None;
                    }
                }

                recompute_datedivider_near(messages, messages.len());
                if !messages.is_empty() {
                    recompute_pad_bottom_at(messages, messages.len() - 1);
                }
            }
        }
        VectorDiff::Insert { index, value } => {
            if index > messages.len() {
                tracing::error!(
                    "Ignoring out-of-range timeline Insert at {} (len {})",
                    index,
                    messages.len()
                );
                return;
            }
            Arc::make_mut(messages).insert(index, cached_from_timeline_item(&value, own_id));
            list_state.splice(index..index, 1);

            recompute_datedivider_near(messages, index);
            if index > 0 {
                recompute_pad_bottom_at(messages, index - 1);
            }

            if let Some((focused, _)) = focused_message {
                if index <= *focused {
                    *focused += 1;
                }
            }

            recompute_show_header_at(messages, index);
            recompute_pad_bottom_at(messages, index);
            recompute_show_header_at(messages, index + 1);
        }
        VectorDiff::Set { index, value } => {
            if index >= messages.len() {
                tracing::error!(
                    "Ignoring out-of-range timeline Set at {} (len {})",
                    index,
                    messages.len()
                );
                return;
            }
            Arc::make_mut(messages)[index] = cached_from_timeline_item(&value, own_id);
            list_state.splice(index..index + 1, 1);

            recompute_datedivider_near(messages, index);
            if index > 0 {
                recompute_pad_bottom_at(messages, index - 1);
            }
            recompute_show_header_at(messages, index);
            recompute_pad_bottom_at(messages, index);
            recompute_show_header_at(messages, index + 1);
        }
        VectorDiff::Remove { index } => {
            if index >= messages.len() {
                tracing::error!(
                    "Ignoring out-of-range timeline Remove at {} (len {})",
                    index,
                    messages.len()
                );
                return;
            }
            Arc::make_mut(messages).remove(index);
            list_state.splice(index..index + 1, 0);

            if let Some((focused, _)) = focused_message {
                if index < *focused {
                    *focused -= 1;
                } else if index == *focused {
                    *focused_message = None;
                }
            }

            recompute_datedivider_near(messages, index);
            if index > 0 {
                recompute_pad_bottom_at(messages, index - 1);
            }
            recompute_show_header_at(messages, index);
        }
        VectorDiff::Truncate { length } => {
            if length > messages.len() {
                tracing::error!(
                    "Ignoring out-of-range timeline Truncate to {} (len {})",
                    length,
                    messages.len()
                );
                return;
            }
            list_state.splice(length..messages.len(), 0);
            Arc::make_mut(messages).truncate(length);

            if let Some((focused, _)) = focused_message {
                if *focused >= length {
                    *focused_message = None;
                }
            }

            recompute_datedivider_near(messages, length);
            if length > 0 {
                recompute_pad_bottom_at(messages, length - 1);
            }
        }
        VectorDiff::Reset { values } => {
            list_state.splice(0..messages.len(), values.len());
            *messages = Arc::new(
                values
                    .iter()
                    .map(|item| cached_from_timeline_item(item, own_id))
                    .collect(),
            );
            *focused_message = None;

            let len = messages.len();
            for i in 0..len {
                let Some((current_item, rest)) = Arc::make_mut(messages)[i..].split_first_mut()
                else {
                    continue;
                };

                current_item.recompute_datedivider_types(rest);
            }
            recompute_grouping_range(messages, 0..len);
        }
    }
}

impl Focusable for TimelineView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TimelineView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let structure = cx.structure();

        let room = self.active_room.clone();
        let room_id = room.room_id().to_owned();

        let messages = self.messages.clone();
        let avatar_cache = self.avatar_cache.clone();
        let image_cache = self.image_cache.clone();

        let map = self.membership_map.borrow().clone();

        let state = self.state.clone();

        let toggle_reaction = self.toggle_reaction();
        let user_id = self.user_id.clone();
        let editing_message = self.editing_message.clone();

        tailwind_div!(size_full, paddings(structure.gap), py_0, flex, flex_col)
            .key_context("Chat")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &SubmitEdit, window, cx| {
                let Some((idx, _)) = &this.focused_message else {
                    return;
                };
                let Some(message) = this.messages.get(*idx) else {
                    return;
                };
                let Some(event_id) = message.owned_event_id() else {
                    return;
                };

                let room = this.active_room.clone();
                let text = this.editing_message.read(cx).text().to_string();

                let task = this
                    .tokio_rt
                    .spawn(async move { room.edit_message(event_id, text).await });

                cx.spawn_in(window, async move |this, cx| match task.await {
                    Ok(Ok(_)) => {
                        let _ = this.update_in(cx, |this, window, cx| {
                            this.cancel_editing(window, cx);
                        });
                    }
                    Ok(Err(err)) => {
                        tracing::error!("Failed to edit message: {err:?}");
                    }
                    Err(err) => {
                        tracing::error!("Task join error: {err:?}");
                    }
                })
                .detach();
            }))
            .on_action(cx.listener(|this, _: &CancelEdit, window, cx| {
                this.cancel_editing(window, cx);
            }))
            .child({
                let focused_message = self.focused_message;
                list(self.list_state.clone(), move |ix, window, cx| {
                    let Some(current) = messages.get(ix) else {
                        return Empty.into_any_element();
                    };
                    let focused = focused_message.is_some_and(|f| f.0 == ix);
                    let editing = focused_message
                        .is_some_and(|f| focused && f.1 && current.is_sent_by(&user_id));

                    current.render(
                        window,
                        cx,
                        toggle_reaction.clone(),
                        &room_id,
                        &map,
                        &avatar_cache,
                        &image_cache,
                        focused,
                        if editing {
                            Some(editing_message.clone())
                        } else {
                            None
                        },
                        &state,
                    )
                })
                .h_full()
                .w_full()
                .pb(structure.gap * 5.0)
            })
            .into_any_element()
    }
}

actions!(
    chat,
    [
        FocusNext,
        FocusPrevious,
        UnfocusInput,
        FocusInput,
        SendMessage,
        EditMessage,
        CancelEdit,
        SubmitEdit,
    ]
);

#[derive(Clone, PartialEq, Deserialize, gpui::Action, schemars::JsonSchema)]
#[action(namespace = chat)]
pub struct FocusInputWithKey {
    pub key: String,
}
