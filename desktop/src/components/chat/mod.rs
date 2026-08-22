use std::sync::Arc;

use deplace_core::{matrix_api::messages::RoomSendingExt, state::AppState};
use gpui::{
    App, AppContext, Context, Entity, FocusHandle, Focusable, ParentElement, Render, Styled, Window,
};
use gpui_component::StyledExt;
use macros::tailwind_div;
use matrix_sdk::{Room, ruma::OwnedRoomId};
use tokio::runtime::Runtime;

use crate::{
    cache::{AvatarCache, ThumbnailCache},
    components::{
        chat::{
            input::{ChatInputView, SendEvent},
            timeline::TimelineView,
        },
        floating_tile,
    },
    things::DeplaceThings,
    view_lru::VisibleLruCache,
};

mod input;
mod message;
mod timeline;

pub use timeline::{
    EditMessage, FocusInput, FocusInputWithKey, FocusNext, FocusPrevious, ScrollOffset,
    UnfocusInput,
};

pub type ChatTimelineCache = VisibleLruCache<OwnedRoomId, ChatView>;

pub struct ChatView {
    input: Entity<ChatInputView>,
    timeline: Entity<TimelineView>,
}

impl ChatView {
    pub fn new(
        state: &AppState,
        cx: &mut Context<Self>,
        window: &mut Window,
        tokio_rt: Arc<Runtime>,
        avatar_cache: AvatarCache,
        image_cache: ThumbnailCache,
        room: Room,
    ) -> Self {
        let timeline = cx.new(|cx| {
            TimelineView::new(
                state,
                cx,
                window,
                tokio_rt.clone(),
                avatar_cache,
                image_cache,
                room.clone(),
            )
        });

        let input = cx.new(|cx| ChatInputView::new(state, window, cx, room.clone()));

        cx.subscribe_in(&input, window, move |_, _, event: &SendEvent, _, _cx| {
            let SendEvent::SendMessage {
                text,
                attachments,
                in_reply_to,
            } = event.clone()
            else {
                tracing::debug!("Ignoring non-send chat input event");
                return;
            };

            let room = room.clone();
            let attachments_empty = attachments.is_empty();

            tokio_rt.spawn(async move {
                if !attachments_empty {
                    let mut iter = attachments.into_iter();

                    if let Some(attachment) = iter.next()
                        && let Err(e) = room
                            .send_deplace_attachment(attachment, in_reply_to.clone())
                            .await
                    {
                        tracing::error!("Failed to send attachment: {}", e);
                    }

                    for attachment in iter {
                        if let Err(e) = room.send_deplace_attachment(attachment, None).await {
                            tracing::error!("Failed to send attachment: {}", e);
                        }
                    }
                }

                if !text.trim().is_empty()
                    && let Err(e) = room
                        .send_message(
                            text,
                            attachments_empty.then(|| in_reply_to.clone()).flatten(),
                        )
                        .await
                {
                    tracing::error!("Failed to send message: {}", e);
                }
            });
        })
        .detach();

        Self { input, timeline }
    }

    /// Inserts `key` at the chat input's cursor position.
    pub fn insert_at_input(&self, key: &str, window: &mut Window, cx: &mut App) {
        self.input.update(cx, |input, cx| {
            input.chat_input.update(cx, |chat_input, cx| {
                chat_input.insert(key, window, cx);
            });
        });
    }
}

impl Focusable for ChatView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for ChatView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        floating_tile(theme, structure).w_full().h_full().child(
            tailwind_div!(
                flex,
                flex_col,
                size_full,
                paddings(structure.small_gap),
                pt_0
            )
            .child(self.timeline.clone())
            .child(self.input.clone()),
        )
    }
}

impl ChatTimelineCache {
    pub fn update_visible_timeline(
        &self,
        cx: &mut impl AppContext,
        update: impl FnOnce(&mut TimelineView, &mut Context<TimelineView>),
    ) {
        let Some(visible) = self.visible() else {
            return;
        };
        visible.update(cx, |chat, cx| chat.timeline.update(cx, update))
    }
}
