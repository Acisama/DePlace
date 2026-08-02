use std::{ops::Range, sync::Arc};

use deplace_core::{
    matrix_api::timeline::{ScrollDirection, TimelineManager},
    state::{AppState, MembershipMap},
};
use futures_util::StreamExt;
use gpui::{
    Context, FollowMode, IntoElement, ListAlignment, ListScrollEvent, ListState, ParentElement,
    Render, Styled, Task, div, list, px,
};
use gpui_component::{StyledExt, red_600};
use macros::tailwind_div;
use matrix_sdk::{
    Room,
    ruma::{OwnedUserId, UserId},
};
use matrix_sdk_ui::{
    eyeball_im::VectorDiff,
    timeline::{TimelineFocus, TimelineItem},
};
use tokio::{runtime::Runtime, sync::watch::Receiver, task::AbortHandle};
use uuid::Uuid;

use crate::{
    components::{
        AvatarCache,
        message::{CachedTimelineItem, cached_from_timeline_item},
    },
    theme::ActiveAppTheme,
    watch_bridge::notify_on_change,
};

pub struct ChatView {
    messages: Vec<CachedTimelineItem>,
    active_room: Receiver<Option<Room>>,
    timeline_manager: TimelineManager,
    timeline_id: Option<Uuid>,
    avatar_cache: AvatarCache,
    membership_map: Receiver<MembershipMap>,
    tokio_rt: Arc<Runtime>,
    current_fetch: Option<AbortHandle>,
    current_scroll: Option<AbortHandle>,
    user_id: OwnedUserId,
    current_updates: Option<Task<()>>,
    list_state: ListState,
}

impl ChatView {
    pub fn new(
        state: &AppState,
        cx: &mut Context<Self>,
        tokio_rt: Arc<Runtime>,
        avatar_cache: AvatarCache,
    ) -> Self {
        let list_state = ListState::new(0, ListAlignment::Bottom, px(500.));
        list_state.set_follow_mode(FollowMode::Tail);
        list_state.set_scroll_handler(cx.listener(|this, event: &ListScrollEvent, _window, cx| {
            this.check_pagination(event.visible_range.clone(), event.count, cx);
        }));

        let membership_map = state.membership_map();

        notify_on_change(membership_map.clone(), cx);

        let mut view = Self {
            timeline_manager: state.timeline_manager.clone(),
            user_id: state.user_device.user_id.clone(),
            messages: Vec::new(),
            avatar_cache,
            membership_map,
            tokio_rt,
            timeline_id: None,
            active_room: state.active_room(),
            current_fetch: None,
            current_scroll: None,
            current_updates: None,
            list_state,
        };

        view.load_active_room(cx);

        cx.spawn({
            let mut active_room = view.active_room.clone();
            async move |this, cx| {
                while active_room.changed().await.is_ok() {
                    if this
                        .update(cx, |view, cx| view.load_active_room(cx))
                        .is_err()
                    {
                        break;
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

        self.current_updates = None;

        self.list_state.splice(0..self.messages.len(), 0);
        self.messages = Vec::new();
        self.timeline_id = None;
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
                        view.messages = initial_messages
                            .iter()
                            .map(|item| cached_from_timeline_item(item, &view.user_id))
                            .collect();
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
        if self.current_scroll.is_some() {
            return;
        }

        let id = self.timeline_id;
        let timeline_manager = self.timeline_manager.clone();

        let task = self.tokio_rt.spawn(async move {
            if let Some(id) = id {
                timeline_manager.scroll_timeline(id, direction).await;
            }
        });
        self.current_scroll = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |view, _cx| {
                view.current_scroll = None;
            });
            if let Err(e) = result {
                tracing::error!("Failed to scroll timeline: {}", e);
            }
        })
        .detach();
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

fn apply_diff(
    messages: &mut Vec<CachedTimelineItem>,
    list_state: &ListState,
    diff: VectorDiff<Arc<TimelineItem>>,
    own_id: &UserId,
) {
    match diff {
        VectorDiff::Append { values } => {
            let start = messages.len();
            messages.extend(
                values
                    .iter()
                    .map(|item| cached_from_timeline_item(item, own_id)),
            );
            list_state.splice(start..start, messages.len() - start);
        }
        VectorDiff::Clear => {
            list_state.splice(0..messages.len(), 0);
            messages.clear();
        }
        VectorDiff::PushFront { value } => {
            messages.insert(0, cached_from_timeline_item(&value, own_id));
            list_state.splice(0..0, 1);
        }
        VectorDiff::PushBack { value } => {
            messages.push(cached_from_timeline_item(&value, own_id));
            list_state.splice(messages.len() - 1..messages.len() - 1, 1);
        }
        VectorDiff::PopFront => {
            if !messages.is_empty() {
                messages.remove(0);
                list_state.splice(0..1, 0);
            }
        }
        VectorDiff::PopBack => {
            if messages.pop().is_some() {
                list_state.splice(messages.len()..messages.len() + 1, 0);
            }
        }
        VectorDiff::Insert { index, value } => {
            messages.insert(index, cached_from_timeline_item(&value, own_id));
            list_state.splice(index..index, 1);
        }
        VectorDiff::Set { index, value } => {
            messages[index] = cached_from_timeline_item(&value, own_id);
            list_state.splice(index..index + 1, 1);
        }
        VectorDiff::Remove { index } => {
            messages.remove(index);
            list_state.splice(index..index + 1, 0);
        }
        VectorDiff::Truncate { length } => {
            list_state.splice(length..messages.len(), 0);
            messages.truncate(length);
        }
        VectorDiff::Reset { values } => {
            list_state.splice(0..messages.len(), values.len());
            *messages = values
                .iter()
                .map(|item| cached_from_timeline_item(item, own_id))
                .collect();
        }
    }
}

impl Render for ChatView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();

        let Some(room) = self.active_room.borrow().clone() else {
            return div().into_any_element();
        };
        let room_id = room.room_id().to_owned();

        let messages = self.messages.clone();
        let avatar_cache = self.avatar_cache.clone();
        let map = self.membership_map.borrow().clone();

        tailwind_div!(size_full, paddings(theme.gap), pt_0, flex, flex_col)
            .child(
                list(self.list_state.clone(), move |ix, _window, cx| {
                    let theme = cx.app_theme();

                    let Some(current) = messages.get(ix) else {
                        return div().into_any_element();
                    };
                    let prev = ix.checked_sub(1).and_then(|prev_ix| messages.get(prev_ix));
                    let next = messages.get(ix + 1);

                    current.render(prev, next, theme, &room_id, &map, &avatar_cache)
                })
                .h_full()
                .w_full(),
            )
            .child(
                div()
                    .w_full()
                    .bg(red_600())
                    .h(theme.structure.header.height)
                    .rounded(theme.inner_border_radius),
            )
            .into_any_element()
    }
}
