use std::{ops::Range, sync::Arc};

use dashmap::DashMap;
use deplace_core::{
    matrix_api::timeline::{ScrollDirection, TimelineManager},
    state::{AppState, MembershipMap},
};
use futures_util::StreamExt;
use gpui::{
    Context, Empty, FocusHandle, Focusable, FollowMode, InteractiveElement, IntoElement,
    ListAlignment, ListScrollEvent, ListState, ParentElement, Render, SharedString, Styled, Task,
    Window, actions, list, px,
};
use gpui_component::StyledExt;
use macros::tailwind_div;
use matrix_sdk::{
    Room,
    ruma::{OwnedEventId, OwnedUserId, UserId},
};
use matrix_sdk_ui::{
    eyeball_im::VectorDiff,
    timeline::{TimelineFocus, TimelineItem},
};
use serde::Deserialize;
use tokio::{runtime::Runtime, sync::watch::Receiver, task::AbortHandle};
use uuid::Uuid;

use crate::{
    cache::{AvatarCache, ThumbnailCache},
    components::message::{CachedTimelineItem, CachedTimelineItemKind, cached_from_timeline_item},
    things::DeplaceThings,
    watch_bridge::notify_on_change,
};

pub struct TimelineView {
    pub messages: Arc<Vec<CachedTimelineItem>>,
    pub focused_message: Option<usize>,
    active_room: Room,
    timeline_manager: TimelineManager,
    timeline_id: Option<Uuid>,

    avatar_cache: AvatarCache,
    image_cache: ThumbnailCache,

    state: AppState,

    membership_map: Receiver<MembershipMap>,
    tokio_rt: Arc<Runtime>,
    current_scroll_up: Option<AbortHandle>,
    current_scroll_down: Option<AbortHandle>,
    reactions_in_flight: Arc<DashMap<(Arc<OwnedEventId>, SharedString), ()>>,
    user_id: OwnedUserId,
    _updates_task: Task<()>,
    pub list_state: ListState,
    focus_handle: FocusHandle,
}

impl TimelineView {
    pub fn new(
        state: &AppState,
        cx: &mut Context<Self>,
        tokio_rt: Arc<Runtime>,
        avatar_cache: AvatarCache,
        image_cache: ThumbnailCache,
        active_room: Room,
    ) -> Self {
        let list_state = ListState::new(0, ListAlignment::Bottom, px(500.));
        list_state.set_follow_mode(FollowMode::Tail);
        list_state.set_scroll_handler(cx.listener(|this, event: &ListScrollEvent, _window, cx| {
            this.check_pagination(event.visible_range.clone(), event.count, cx);
        }));

        let membership_map = state.membership_map();
        let settings = state.settings();
        let timeline_manager = state.timeline_manager();

        notify_on_change(membership_map.clone(), cx);
        notify_on_change(avatar_cache.subscribe(), cx);
        notify_on_change(image_cache.subscribe(), cx);

        notify_on_change(settings.watch_data_size_unit(), cx);
        notify_on_change(settings.watch_system_messages_to_show(), cx);

        let room_id = active_room.room_id().to_owned();

        tracing::debug!("Loading timeline for room {}", room_id);

        let clone = active_room.clone();
        let task = tokio_rt.spawn(async move {
            timeline_manager
                .get_messages(
                    &clone,
                    TimelineFocus::Live {
                        hide_threaded_events: false,
                    },
                )
                .await
        });

        let updates_task = cx.spawn(async move |this, cx| {
            let result = task.await;
            let outcome = this.update(cx, |view, cx| {
                let update_stream = match result {
                    Ok(Ok((initial_messages, update_stream, id))) => {
                        tracing::debug!(
                            "Loaded timeline for room {}: {} messages",
                            room_id,
                            initial_messages.len()
                        );

                        let mut cached_messages: Vec<CachedTimelineItem> = initial_messages
                            .iter()
                            .map(|item| cached_from_timeline_item(item, &view.user_id))
                            .collect();

                        for i in 0..cached_messages.len() {
                            let Some((current_item, rest)) = cached_messages[i..].split_first_mut()
                            else {
                                continue;
                            };

                            current_item.recompute_datedivider_types(rest);
                        }

                        view.timeline_id = Some(id);
                        view.messages = Arc::new(cached_messages);
                        let len = view.messages.len();
                        recompute_grouping_range(&mut view.messages, 0..len);
                        view.list_state.splice(0..0, view.messages.len());
                        view.list_state.set_follow_mode(FollowMode::Tail);
                        Some(update_stream)
                    }
                    Ok(Err(e)) => {
                        tracing::error!("Failed to load timeline: {e:?}");
                        None
                    }
                    Err(e) if e.is_cancelled() => None,
                    Err(e) => {
                        tracing::error!("Timeline fetch task failed: {e:?}");
                        None
                    }
                };
                cx.notify();
                update_stream
            });

            let mut update_stream = match outcome {
                Ok(Some(pair)) => pair,
                Ok(None) => return,
                Err(e) => {
                    tracing::error!(
                        "Stopping timeline load for room {}, ChatView entity is gone: {:?}",
                        room_id,
                        e
                    );
                    return;
                }
            };

            while let Some(diffs) = update_stream.next().await {
                let updated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    this.update(cx, |view, cx| {
                        for diff in diffs {
                            apply_diff(&mut view.messages, &view.list_state, diff, &view.user_id);
                        }
                        cx.notify();
                    })
                }));

                match updated {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        tracing::debug!(
                            "Stopping timeline updates for room {}, entity is gone: {:?}",
                            room_id,
                            e
                        );
                        break;
                    }
                    Err(e) => {
                        tracing::error!(
                            "Panic while applying timeline diff for room {}: {:?}",
                            room_id,
                            e
                        );
                    }
                }
            }
            tracing::warn!("Timeline update stream ended for room {}", room_id);
        });

        Self {
            timeline_manager: state.timeline_manager(),
            user_id: state.user_device().user_id.clone(),
            messages: Arc::new(Vec::new()),
            state: state.clone(),
            focused_message: None,
            avatar_cache,
            image_cache,
            membership_map,
            tokio_rt,
            timeline_id: None,
            active_room,
            current_scroll_up: None,
            current_scroll_down: None,
            reactions_in_flight: Arc::new(DashMap::new()),
            _updates_task: updates_task,
            list_state,
            focus_handle: cx.focus_handle(),
        }
    }

    fn scroll(&mut self, cx: &mut Context<Self>, direction: ScrollDirection) {
        let current_scroll = match direction {
            ScrollDirection::Up => &mut self.current_scroll_up,
            ScrollDirection::Down => &mut self.current_scroll_down,
        };
        if current_scroll.is_some() {
            return;
        }

        // Timeline hasn't finished its initial load yet - nothing to scroll yet.
        let Some(id) = self.timeline_id else {
            return;
        };
        let timeline_manager = self.timeline_manager.clone();

        let task = self.tokio_rt.spawn(async move {
            timeline_manager.scroll_timeline(id, direction).await;
        });
        *current_scroll = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            let result = task.await;
            if let Err(e) = this.update(cx, |view, _cx| {
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
            if let Err(e) = result {
                tracing::error!("Failed to scroll timeline: {}", e);
            }
        })
        .detach();
    }

    fn toggle_reaction(&self) -> impl Fn(Arc<OwnedEventId>, SharedString) + Clone + 'static {
        let tokio_rt = self.tokio_rt.clone();
        let timeline_manager = self.timeline_manager.clone();
        let in_flight = self.reactions_in_flight.clone();
        let timeline_id = self.timeline_id;

        move |event_id: Arc<OwnedEventId>, reaction: SharedString| {
            let Some(timeline_id) = timeline_id else {
                return;
            };

            let key = (event_id.clone(), reaction.clone());
            if in_flight.insert(key.clone(), ()).is_some() {
                return;
            }

            let timeline_manager = timeline_manager.clone();
            let in_flight = in_flight.clone();
            tokio_rt.spawn(async move {
                timeline_manager
                    .toggle_reaction(timeline_id, (*event_id).clone(), &reaction)
                    .await;
                in_flight.remove(&key);
            });
        }
    }

    fn check_pagination(&mut self, range: Range<usize>, len: usize, cx: &mut Context<Self>) {
        const EDGE_THRESHOLD: usize = 10;

        if range.start < EDGE_THRESHOLD {
            self.scroll(cx, ScrollDirection::Up);
        }
        if len.saturating_sub(range.end) < EDGE_THRESHOLD {
            self.scroll(cx, ScrollDirection::Down);
        }
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
        }
        VectorDiff::PushFront { value } => {
            Arc::make_mut(messages).insert(0, cached_from_timeline_item(&value, own_id));
            list_state.splice(0..0, 1);

            if let Some((current_item, rest)) = Arc::make_mut(messages).split_first_mut() {
                current_item.recompute_datedivider_types(rest);
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

                recompute_show_header_at(messages, 0);
            }
        }
        VectorDiff::PopBack => {
            if Arc::make_mut(messages).pop().is_some() {
                list_state.splice(messages.len()..messages.len() + 1, 0);

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

        let on_toggle_reaction = self.toggle_reaction();

        let state = self.state.clone();

        tailwind_div!(size_full, paddings(structure.gap), py_0, flex, flex_col)
            .key_context("Chat")
            .track_focus(&self.focus_handle)
            .child({
                let focused_message = self.focused_message;
                list(self.list_state.clone(), move |ix, window, cx| {
                    let theme = cx.app_theme();
                    let structure = cx.structure();
                    let importantpaths = cx.important_paths();

                    let Some(current) = messages.get(ix) else {
                        return Empty.into_any_element();
                    };
                    let focused = focused_message.is_some_and(|f| f == ix);

                    current.render(
                        window,
                        theme,
                        structure,
                        &room_id,
                        &map,
                        &avatar_cache,
                        &image_cache,
                        focused,
                        on_toggle_reaction.clone(),
                        &state,
                        importantpaths,
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
        SendMessage
    ]
);

#[derive(Clone, PartialEq, Deserialize, gpui::Action, schemars::JsonSchema)]
#[action(namespace = chat)]
pub struct FocusInputWithKey {
    pub key: String,
}
