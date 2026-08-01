use std::{ops::Range, sync::Arc};

use deplace_core::{
    matrix_api::timeline::{Messages, ScrollDirection, TimelineManager},
    state::AppState,
};
use gpui::{
    Context, FollowMode, IntoElement, ListAlignment, ListScrollEvent, ListState, ParentElement,
    Render, Styled, div, list, px,
};
use gpui_component::{StyledExt, red_600};
use matrix_sdk::Room;
use matrix_sdk_ui::timeline::{TimelineFocus, TimelineItem, TimelineItemKind};
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
    tokio_rt: Arc<Runtime>,
    current_fetch: Option<AbortHandle>,
    list_state: ListState,
    rendered_len: usize,
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

        let mut view = Self {
            timeline_manager: state.timeline_manager.clone(),
            messages: None,
            avatar_cache,
            tokio_rt,
            timeline_id: None,
            active_room: state.active_room(),
            current_fetch: None,
            list_state,
            rendered_len: 0,
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

        let Some(room) = self.active_room.borrow_and_update().clone() else {
            self.messages = None;
            cx.notify();
            return;
        };

        let manager = self.timeline_manager.clone();
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
                view.current_fetch = None;
                match result {
                    Ok(Ok((receiver, id))) => {
                        notify_on_change(receiver.clone(), cx); // re-render on new events too
                        view.messages = Some(receiver);
                        view.timeline_id = Some(id);
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
        if self.current_fetch.is_some() {
            return;
        }

        let id = self.timeline_id;
        let timeline_manager = self.timeline_manager.clone();
        let tokio_rt = self.tokio_rt.clone();
        cx.spawn(async move |this, cx| {
            tokio_rt.spawn(async move {
                if let Some(id) = id {
                    timeline_manager.scroll_timeline(id, direction).await;
                }
            });
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

impl Render for ChatView {
    fn render(&mut self, _window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();

        let messages = self
            .messages
            .clone()
            .map(|r| r.borrow().clone())
            .unwrap_or_default();
        let len = messages.len();

        if len != self.rendered_len {
            self.list_state.splice(0..self.rendered_len, len);
            self.rendered_len = len;
        }

        div()
            .size_full()
            .paddings(theme.gap)
            .gap(theme.gap)
            .flex()
            .flex_col()
            .child(
                list(self.list_state.clone(), move |ix, _window, cx| {
                    let theme = cx.app_theme();
                    messages
                        .get(ix)
                        .map(|t| render_timeline_item(t.clone(), theme).into_any_element())
                        .unwrap_or_else(|| div().into_any_element())
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
    }
}
