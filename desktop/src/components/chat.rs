use std::{ops::Range, sync::Arc};

use deplace_core::{
    matrix_api::timeline::{Messages, ScrollDirection, TimelineManager},
    state::{AppState, MembershipMap},
};
use gpui::{
    Context, FollowMode, IntoElement, ListAlignment, ListScrollEvent, ListState, ParentElement,
    Render, Styled, div, list, px,
};
use gpui_component::{StyledExt, red_600};
use matrix_sdk::Room;
use matrix_sdk_ui::timeline::TimelineFocus;
use tokio::{runtime::Runtime, sync::watch::Receiver, task::AbortHandle};
use uuid::Uuid;

use crate::{
    components::{AvatarCache, timeline::render_timeline_item},
    theme::ActiveAppTheme,
    watch_bridge::notify_on_change,
};

pub struct ChatView {
    messages: Option<Receiver<Messages>>,
    active_room: Receiver<Option<Room>>,
    timeline_manager: TimelineManager,
    timeline_id: Option<Uuid>,
    avatar_cache: AvatarCache,
    membership_map: Receiver<MembershipMap>,
    tokio_rt: Arc<Runtime>,
    current_fetch: Option<AbortHandle>,
    current_scroll: Option<AbortHandle>,
    list_state: ListState,
    rendered_messages: Messages,
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
            messages: None,
            avatar_cache,
            membership_map,
            tokio_rt,
            timeline_id: None,
            active_room: state.active_room(),
            current_fetch: None,
            current_scroll: None,
            list_state,
            rendered_messages: Messages::default(),
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

        // Clear immediately rather than leaving the previous room's messages on screen
        // while the new room's timeline loads.
        self.messages = None;
        self.timeline_id = None;
        self.list_state.splice(0..self.rendered_messages.len(), 0);
        self.rendered_messages = Messages::default();
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

        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |view, cx| {
                // A fetch that should have been aborted can still resolve successfully if it
                // finished right as a newer room was selected. Applying it here would silently
                // overwrite the newer room's (still in-flight) selection with stale data, so
                // discard anything that isn't for the room we're currently showing.
                let is_current = view.active_room.borrow().as_ref().map(|r| r.room_id())
                    == Some(expected_room_id.as_ref());
                if !is_current {
                    tracing::debug!(
                        "Discarding stale timeline load for room {}",
                        expected_room_id
                    );
                    return;
                }

                view.current_fetch = None;
                match result {
                    Ok(Ok((receiver, id))) => {
                        tracing::debug!(
                            "Loaded timeline for room {}: {} messages",
                            expected_room_id,
                            receiver.borrow().len()
                        );
                        notify_on_change(receiver.clone(), cx); // re-render on new events too
                        view.messages = Some(receiver);
                        view.timeline_id = Some(id);
                        // New room: always land on the newest message, regardless of
                        // whatever scroll/follow state the previous room left behind.
                        view.list_state.set_follow_mode(FollowMode::Tail);
                    }
                    Ok(Err(e)) => tracing::error!("Failed to load timeline: {e:?}"),
                    Err(e) if e.is_cancelled() => {}
                    Err(e) => tracing::error!("Timeline fetch task failed: {e:?}"),
                }
                cx.notify();
            });
        })
        .detach();
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

    /// Tell `ListState` precisely what changed, so it can preserve scroll position.
    ///
    /// Splicing the whole `0..old_len` range on every update (as if the entire list were
    /// replaced) makes `ListState` reset scroll position to the top on every change, since it
    /// can't tell that the items it's already measured are still there. Detecting a pure
    /// prepend (older messages paginated in) or append (new live message) and giving it the
    /// exact sub-range that changed lets it shift/preserve the current scroll position instead.
    fn splice_messages(&mut self, messages: &Messages) {
        let old_len = self.rendered_messages.len();
        let new_len = messages.len();

        if old_len == new_len {
            return;
        }

        let unchanged_at =
            |a: &Messages, a_ix: usize, b: &Messages, b_ix: usize| match (a.get(a_ix), b.get(b_ix))
            {
                (Some(x), Some(y)) => Arc::ptr_eq(x, y),
                _ => false,
            };

        let prepended = new_len > old_len
            && old_len > 0
            && unchanged_at(messages, new_len - old_len, &self.rendered_messages, 0)
            && unchanged_at(messages, new_len - 1, &self.rendered_messages, old_len - 1);

        let appended = new_len > old_len
            && old_len > 0
            && unchanged_at(messages, 0, &self.rendered_messages, 0)
            && unchanged_at(messages, old_len - 1, &self.rendered_messages, old_len - 1);

        if prepended {
            self.list_state.splice(0..0, new_len - old_len);
        } else if appended {
            self.list_state.splice(old_len..old_len, new_len - old_len);
        } else {
            self.list_state.splice(0..old_len, new_len);
        }

        self.rendered_messages = messages.clone();
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

impl Render for ChatView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();

        let Some(room) = self.active_room.borrow().clone() else {
            return div().into_any_element();
        };
        let room_id = room.room_id().to_owned();

        let messages = self
            .messages
            .clone()
            .map(|r| r.borrow().clone())
            .unwrap_or_default();
        self.splice_messages(&messages);

        let avatar_cache = self.avatar_cache.clone();
        let map = self.membership_map.borrow().clone();

        div()
            .size_full()
            .paddings(theme.gap)
            .gap(theme.gap)
            .flex()
            .flex_col()
            .child(
                list(self.list_state.clone(), move |ix, _window, cx| {
                    let theme = cx.app_theme();

                    let Some(current) = messages.get(ix) else {
                        return div().into_any_element();
                    };
                    let prev = ix.checked_sub(1).and_then(|prev_ix| messages.get(prev_ix));
                    let next = ix.checked_sub(1).and_then(|next_ix| messages.get(next_ix));

                    render_timeline_item(
                        current.clone(),
                        prev.cloned(),
                        next.cloned(),
                        theme,
                        &room_id,
                        &map,
                        &avatar_cache,
                    )
                    .into_any_element()
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
