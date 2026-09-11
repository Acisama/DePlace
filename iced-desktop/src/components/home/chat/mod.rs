use std::future;

use deplace_core::PaginationDirection;
use header::{Header, HeaderAction, HeaderMessage};
use iced::futures::{StreamExt, stream};
use iced::widget::text_editor;
use input::{ChatInput, InputAction, InputMessage};
use macros::iced_cache;
use matrix_sdk_ui::timeline::{
    DateDividerMode, TimelineBuilder, TimelineFocus, TimelineReadReceiptTracking,
};
use timeline::{ChatTimeline, TimelineAction, ToTimelineItem};

use crate::common::*;
pub(super) mod empty;
mod header;
mod input;
mod timeline;

pub use timeline::TimelineMessage;

#[derive(Debug, Clone)]
pub enum ChatMessage {
    Header(HeaderMessage),
    Timeline(TimelineMessage),
    Input(InputMessage),
}

pub enum ChatAction {
    NeedsMedia(NeedsMedia),
    Run(Task<()>),
    TimelineScroll {
        direction: PaginationDirection,
        task: Task<bool>,
    },
}

#[iced_cache(Clone)]
pub struct Chat {
    #[hash]
    header: Header,
    #[hash]
    timeline: ChatTimeline,
    input: ChatInput,

    pub room_id: OwnedRoomId,
}

impl Chat {
    pub fn new(state: &AppState, room: Room) -> (Self, Task<(OwnedRoomId, TimelineMessage)>) {
        let builder = TimelineBuilder::new(&room)
            .with_date_divider_mode(DateDividerMode::Daily)
            .with_focus(TimelineFocus::Live {
                hide_threaded_events: false,
            })
            .track_read_marker_and_receipts(TimelineReadReceiptTracking::AllEvents)
            .add_failed_to_parse(true);

        let room_id = room.room_id().to_owned();

        let avatar_cache = state.avatar_cache().clone();
        let thumbnail_cache = state.thumbnail_cache().clone();
        let video_cache = state.video_cache().clone();

        let room_id_clone = room_id.clone();
        let room_clone = room.clone();
        let stream = stream::once(async move {
            tracing::debug!("Building timeline for room {}", room_id_clone);
            let timeline = match builder.build().await {
                Ok(t) => Arc::new(t),
                Err(e) => {
                    tracing::error!(
                        "Failed to build timeline for room {}: {:?}",
                        room_id_clone,
                        e
                    );
                    return stream::pending().left_stream(); // never resolves; room stays empty
                }
            };
            let (initial, updates) = timeline.subscribe().await;
            tracing::debug!(
                "Subscribed to timeline for room {} with {} initial items",
                room_id_clone,
                initial.len()
            );

            if initial.len() < 100 {
                match timeline.paginate_backwards(30).await {
                    Ok(reached_start) => tracing::debug!(
                        "Paginated room {} backwards (reached_start={})",
                        room_id_clone,
                        reached_start
                    ),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to paginate room {} backwards: {:?}",
                            room_id_clone,
                            e
                        )
                    }
                }
            }

            let power_levels = Arc::new(room_clone.power_levels_or_default().await);

            let initial = Arc::new(
                initial
                    .into_iter()
                    .map(|m| {
                        (
                            m.unique_id().0.clone(),
                            Arc::new(m.convert(
                                &avatar_cache,
                                &thumbnail_cache,
                                &video_cache,
                                room_id_clone.clone(),
                            )),
                        )
                    })
                    .collect(),
            );

            stream::once(future::ready(TimelineMessage::Loaded {
                timeline,
                initial,
                power_levels,
            }))
            .chain(updates.map(TimelineMessage::Diffs))
            .right_stream()
        })
        .flatten();

        (
            Self {
                header: Header::new(state, &room),
                timeline: ChatTimeline::new(&room, state),
                input: ChatInput::new(&room),

                room_id: room.room_id().to_owned(),
            },
            Task::stream(stream).map(move |msg| (room_id.clone(), msg)),
        )
    }

    pub fn get_input_pointer(&self) -> *mut text_editor::Content {
        self.input.get_raw_content_pointer()
    }

    pub fn focus_input(&self) -> Task<()> {
        self.input.focus()
    }

    pub fn set_timeline_scroll_finished(&mut self, direction: PaginationDirection, finished: bool) {
        self.timeline.set_scroll_finished(direction, finished);
    }

    pub fn touch_media(&mut self, media: &MediaLoaded) {
        self.timeline.touch_media(media);
    }
}

impl IcedWidget<ChatMessage, ChatAction> for Chat {
    fn update(&mut self, msg: ChatMessage) -> Option<ChatAction> {
        if let ChatMessage::Timeline(TimelineMessage::Loaded { timeline, .. }) = &msg {
            self.input.timeline = Some(timeline.clone())
        }

        match msg {
            ChatMessage::Header(msg) => {
                if let Some(action) = self.header.update(msg) {
                    match action {
                        HeaderAction::NeedsMedia(media) => Some(ChatAction::NeedsMedia(media)),
                        HeaderAction::Run(task) => Some(ChatAction::Run(task)),
                    }
                } else {
                    None
                }
            }
            ChatMessage::Timeline(msg) => {
                if let Some(action) = self.timeline.update(msg) {
                    match action {
                        TimelineAction::SetIsReplyingTo { event_id, message } => {
                            self.input.set_replies_to(message, event_id);
                            None
                        }
                        TimelineAction::Run(task) => Some(ChatAction::Run(task)),
                        TimelineAction::NeedsMedia(needs_media) => {
                            Some(ChatAction::NeedsMedia(needs_media))
                        }
                        TimelineAction::Scroll { direction, task } => {
                            Some(ChatAction::TimelineScroll { direction, task })
                        }
                    }
                } else {
                    None
                }
            }
            ChatMessage::Input(msg) => {
                if let Some(action) = self.input.update(msg) {
                    match action {
                        InputAction::RemoveReplying(task) => {
                            self.timeline.remove_replying();
                            Some(ChatAction::Run(task))
                        }
                    }
                } else {
                    None
                }
            }
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ChatMessage> {
        w::column![
            w::lazy(self.header.clone(), move |header| {
                header.view(theme, structure).map(ChatMessage::Header)
            }),
            floating_tile(
                theme,
                structure,
                w::column![
                    w::lazy(self.timeline.clone(), move |timeline| timeline
                        .view(theme, structure)
                        .map(ChatMessage::Timeline)),
                    w::lazy(self.input.clone(), move |input| input
                        .view(theme, structure)
                        .map(ChatMessage::Input)),
                ]
                .padding(Padding {
                    top: structure.border_thickness,
                    left: structure.outer_border_radius,
                    right: structure.outer_border_radius,
                    bottom: structure.small_gap + structure.border_thickness,
                })
                .height(Fill)
                .width(Fill)
            )
        ]
        .height(Fill)
        .height(Fill)
        .spacing(structure.small_gap)
        .into()
    }
}
