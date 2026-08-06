use std::{ops::Range, rc::Rc, sync::Arc};

use dashmap::DashMap;
use deplace_core::{
    NameExt,
    matrix_api::timeline::{ScrollDirection, TimelineManager},
    state::{AppState, MembershipMap},
};
use futures_util::StreamExt;
use gpui::{
    App, AppContext, ClipboardItem, Context, Empty, FocusHandle, Focusable, FollowMode,
    InteractiveElement, IntoElement, ListAlignment, ListScrollEvent, ListState, MouseButton,
    ParentElement, Render, SharedString, Styled, Task, Window, actions, div, list, px,
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
    components::{
        AvatarCache, CustomStyles,
        cache::ThumbnailCache,
        message::{
            CachedTimelineItem, HoverState, SelectionState, TextCoord, cached_from_timeline_item,
            format_selection,
        },
    },
    theme::{ActiveAppTheme, StructureExt},
    watch_bridge::notify_on_change,
};

pub struct ChatView {
    messages: Arc<Vec<CachedTimelineItem>>,
    focused_message: Option<usize>,
    active_room: Receiver<Option<Room>>,
    timeline_manager: TimelineManager,
    timeline_id: Option<Uuid>,

    avatar_cache: AvatarCache,
    image_cache: ThumbnailCache,

    membership_map: Receiver<MembershipMap>,
    tokio_rt: Arc<Runtime>,
    current_fetch: Option<AbortHandle>,
    current_scroll_up: Option<AbortHandle>,
    current_scroll_down: Option<AbortHandle>,
    reactions_in_flight: Arc<DashMap<(Arc<OwnedEventId>, SharedString), ()>>,
    user_id: OwnedUserId,
    current_updates: Option<Task<()>>,
    list_state: ListState,
    focus_handle: FocusHandle,
    /// Which rich-text run (link/mention) is currently hovered, if any - see `HoverState`.
    hovered_link: Option<SharedString>,
    /// The current cross-element/cross-message text selection, if any - see `SelectionState`.
    selection: Option<(TextCoord, TextCoord)>,
    /// Whether the mouse is currently held down dragging a selection - `selection` alone stays
    /// `Some` after release (so the highlight persists), so this is what tells a mouse-move
    /// whether to actually extend it.
    dragging_selection: bool,
}

impl ChatView {
    pub fn new(
        state: &AppState,
        cx: &mut Context<Self>,
        tokio_rt: Arc<Runtime>,
        avatar_cache: AvatarCache,
        image_cache: ThumbnailCache,
    ) -> Self {
        let list_state = ListState::new(0, ListAlignment::Bottom, px(500.));
        list_state.set_follow_mode(FollowMode::Tail);
        list_state.set_scroll_handler(cx.listener(|this, event: &ListScrollEvent, _window, cx| {
            this.check_pagination(event.visible_range.clone(), event.count, cx);
        }));
        let active_room = state.active_room();

        let membership_map = state.membership_map();

        notify_on_change(active_room.clone(), cx);
        notify_on_change(membership_map.clone(), cx);
        notify_on_change(avatar_cache.subscribe(), cx);
        notify_on_change(image_cache.subscribe(), cx);

        let mut view = Self {
            timeline_manager: state.timeline_manager.clone(),
            user_id: state.user_device.user_id.clone(),
            messages: Arc::new(Vec::new()),
            focused_message: None,
            avatar_cache,
            image_cache,
            membership_map,
            tokio_rt,
            timeline_id: None,
            active_room,
            current_fetch: None,
            current_scroll_up: None,
            current_scroll_down: None,
            reactions_in_flight: Arc::new(DashMap::new()),
            current_updates: None,
            list_state,
            focus_handle: cx.focus_handle(),
            hovered_link: None,
            selection: None,
            dragging_selection: false,
        };

        view.load_active_room(cx);

        cx.spawn({
            let mut active_room = view.active_room.clone();
            async move |this, cx| {
                while active_room.changed().await.is_ok() {
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        this.update(cx, |view, cx| view.load_active_room(cx))
                    })) {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => break,
                        Err(e) => {
                            tracing::error!(
                                "Panic while loading active room, will keep listening for room changes: {:?}",
                                e
                            );
                        }
                    }
                }
            }
        })
        .detach();

        view
    }

    fn load_active_room(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.current_fetch.take() {
            handle.abort();
        }
        if let Some(handle) = self.current_scroll_up.take() {
            handle.abort();
        }
        if let Some(handle) = self.current_scroll_down.take() {
            handle.abort();
        }

        self.current_updates = None;

        self.list_state.splice(0..self.messages.len(), 0);
        self.messages = Arc::new(Vec::new());
        self.timeline_id = None;
        self.focused_message = None;
        cx.notify();

        let Some(room) = self.active_room.borrow_and_update().clone() else {
            return;
        };

        tracing::debug!("Loading timeline for room {}", room.room_id());

        let manager = self.timeline_manager.clone();
        let expected_room_id = room.room_id().to_owned();
        let task = self.tokio_rt.spawn(async move {
            manager
                .get_messages(
                    &room,
                    TimelineFocus::Live {
                        hide_threaded_events: false,
                    },
                )
                .await
        });
        self.current_fetch = Some(task.abort_handle());

        let update_task = cx.spawn(async move |this, cx| {
            let result = task.await;
            let outcome = this.update(cx, |view, cx| {
                let is_current = view.active_room.borrow().as_ref().map(|r| r.room_id())
                    == Some(expected_room_id.as_ref());
                if !is_current {
                    tracing::debug!(
                        "Discarding stale timeline load for room {}",
                        expected_room_id
                    );
                    return None;
                }

                view.current_fetch = None;
                let update_stream = match result {
                    Ok(Ok((initial_messages, update_stream, id))) => {
                        tracing::debug!(
                            "Loaded timeline for room {}: {} messages",
                            expected_room_id,
                            initial_messages.len()
                        );
                        view.messages = Arc::new(
                            initial_messages
                                .iter()
                                .map(|item| cached_from_timeline_item(item, &view.user_id))
                                .collect(),
                        );
                        let len = view.messages.len();
                        recompute_grouping_range(&mut view.messages, 0..len);
                        view.list_state.splice(0..0, view.messages.len());
                        view.timeline_id = Some(id);
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

            let Ok(Some(mut update_stream)) = outcome else {
                return;
            };

            while let Some(diffs) = update_stream.next().await {
                let updated = this.update(cx, |view, cx| {
                    for diff in diffs {
                        apply_diff(&mut view.messages, &view.list_state, diff, &view.user_id);
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        });
        self.current_updates = Some(update_task);
    }

    fn scroll(&mut self, cx: &mut Context<Self>, direction: ScrollDirection) {
        let current_scroll = match direction {
            ScrollDirection::Up => &mut self.current_scroll_up,
            ScrollDirection::Down => &mut self.current_scroll_down,
        };
        if current_scroll.is_some() {
            return;
        }

        let id = self.timeline_id;
        let timeline_manager = self.timeline_manager.clone();

        let task = self.tokio_rt.spawn(async move {
            if let Some(id) = id {
                timeline_manager.scroll_timeline(id, direction).await;
            }
        });
        *current_scroll = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |view, _cx| {
                let current_scroll = match direction {
                    ScrollDirection::Up => &mut view.current_scroll_up,
                    ScrollDirection::Down => &mut view.current_scroll_down,
                };
                *current_scroll = None;
            });
            if let Err(e) = result {
                tracing::error!("Failed to scroll timeline: {}", e);
            }
        })
        .detach();
    }

    fn toggle_reaction(&self) -> impl Fn(Arc<OwnedEventId>, SharedString) + Clone + 'static {
        let tokio_rt = self.tokio_rt.clone();
        let timeline_manager = self.timeline_manager.clone();
        let timeline_id = self.timeline_id;
        let in_flight = self.reactions_in_flight.clone();

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
            recompute_grouping_range(messages, 0..len);
        }
    }
}

impl Focusable for ChatView {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let structure = cx.structure();

        let Some(room) = self.active_room.borrow().clone() else {
            return div().into_any_element();
        };
        let room_id = room.room_id().to_owned();

        let messages = self.messages.clone();
        let avatar_cache = self.avatar_cache.clone();
        let image_cache = self.image_cache.clone();

        let map = self.membership_map.borrow().clone();

        let on_toggle_reaction = self.toggle_reaction();

        let self_entity = cx.entity();
        let hover = HoverState::new(self.hovered_link.clone(), move |key, cx| {
            self_entity.update(cx, |view, cx| {
                if view.hovered_link != key {
                    view.hovered_link = key;
                    cx.notify();
                }
            });
        });

        // `finish` is shared: `SelectionState` calls it from mouse-up on whichever text
        // element the drag ended over, and the outer container below calls it too, as a
        // safety net for a drag that's released outside any message's text bounds entirely
        // (nothing would otherwise finalize/copy that selection).
        let finish_selection: Rc<dyn Fn(&mut App)> = {
            let self_entity = cx.entity();
            let map = map.clone();
            let room_id = room_id.clone();
            Rc::new(move |cx: &mut App| {
                self_entity.update(cx, |view, cx| {
                    // Mouse-up always ends the drag, whether or not there ends up being a
                    // selection to keep - otherwise a stray extra `finish` call (e.g. the
                    // top-level safety net firing alongside an element's own handler) would
                    // leave `dragging_selection` stuck true.
                    view.dragging_selection = false;

                    let Some((anchor, cursor)) = view.selection else {
                        return;
                    };

                    // A zero-length "selection" was just a click - nothing to keep visible.
                    // A real drag stays highlighted after release, like any normal text
                    // selection; it's only replaced by starting a new drag elsewhere (`start`
                    // already overwrites `view.selection` for that) or cleared explicitly.
                    if anchor == cursor {
                        view.selection = None;
                        cx.notify();
                        return;
                    }

                    let text = format_selection(&view.messages, anchor, cursor, &|id| {
                        map.get(&room_id)
                            .and_then(|m| m.get(id))
                            .map(|member| member.get_name().into())
                            .unwrap_or_else(|| "Unknown".into())
                    });
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                });
            })
        };

        let selection = {
            let self_entity = cx.entity();
            let start = move |coord: TextCoord, cx: &mut App| {
                self_entity.update(cx, |view, cx| {
                    view.selection = Some((coord, coord));
                    view.dragging_selection = true;
                    cx.notify();
                });
            };

            let self_entity = cx.entity();
            let extend = move |coord: TextCoord, cx: &mut App| {
                self_entity.update(cx, |view, cx| {
                    let anchor = view.selection.map_or(coord, |(anchor, _)| anchor);
                    let updated = Some((anchor, coord));
                    if view.selection != updated {
                        view.selection = updated;
                        cx.notify();
                    }
                });
            };

            let finish_selection = finish_selection.clone();
            let finish = move |cx: &mut App| finish_selection(cx);

            SelectionState::new(self.selection, self.dragging_selection, start, extend, finish)
        };

        tailwind_div!(size_full, paddings(structure.gap), pt_0, flex, flex_col)
            .key_context("Chat")
            .on_mouse_up(MouseButton::Left, {
                let finish_selection = finish_selection.clone();
                move |_event, _window, cx| finish_selection(cx)
            })
            .on_action(cx.listener(|this, FocusNext, _, cx| {
                tracing::debug!("Focusing next message");
                let mut new_focus = match this.focused_message {
                    Some(focus) => focus + 1,
                    None if !this.messages.is_empty() => this.messages.len() - 1,
                    None => return,
                };

                while let Some(item) = this.messages.get(new_focus) {
                    if item.is_user_message() {
                        this.focused_message = Some(new_focus);
                        this.list_state.set_follow_mode(FollowMode::Normal);
                        this.list_state.scroll_to_reveal_item(new_focus);
                        cx.notify();
                        tracing::debug!("Focused message {}", new_focus);
                        return;
                    }
                    new_focus += 1;
                }
                tracing::debug!("Didn't find new message to focus");
                // if no new user message to focus is found, we don't change focus
                // TODO: Focus the message input instead
            }))
            .on_action(cx.listener(|this, FocusPrevious, _, cx| {
                tracing::debug!("Focusing previous message");
                let mut new_focus = match this.focused_message {
                    Some(focus) => focus.saturating_sub(1),
                    None if !this.messages.is_empty() => this.messages.len().saturating_sub(1),
                    None => return,
                };

                while let Some(item) = this.messages.get(new_focus) {
                    if item.is_user_message() {
                        this.focused_message = Some(new_focus);
                        this.list_state.set_follow_mode(FollowMode::Normal);
                        this.list_state.scroll_to_reveal_item(new_focus);
                        cx.notify();
                        tracing::debug!("Focused message {}", new_focus);
                        return;
                    }
                    if new_focus == 0 {
                        // TODO: reached top of loaded chat messages, load more
                        return;
                    }
                    new_focus -= 1;
                }
                tracing::debug!("Didn't find new message to focus");
                // if no new user message to focus is found, we don't change focus
            }))
            .track_focus(&self.focus_handle)
            .child({
                let focused_message = self.focused_message;
                list(self.list_state.clone(), move |ix, window, cx| {
                    let theme = cx.app_theme();
                    let structure = cx.structure();

                    let Some(current) = messages.get(ix) else {
                        return Empty.into_any_element();
                    };
                    let focused = focused_message.is_some_and(|f| f == ix);

                    current.render(
                        ix,
                        window,
                        theme,
                        structure,
                        &room_id,
                        &map,
                        &avatar_cache,
                        &image_cache,
                        focused,
                        &hover,
                        &selection,
                        on_toggle_reaction.clone(),
                    )
                })
                .h_full()
                .w_full()
                .pb(structure.gap * 5.0)
            })
            .into_any_element()
    }
}

actions!(chat, [FocusNext, FocusPrevious, UnfocusInput, FocusInput]);

#[derive(Clone, PartialEq, Deserialize, gpui::Action, schemars::JsonSchema)]
#[action(namespace = chat)]
pub struct FocusInputWithKey {
    pub key: String,
}
