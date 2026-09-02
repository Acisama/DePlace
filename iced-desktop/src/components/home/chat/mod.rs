use std::future;

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
    Run(Task<()>),
    None,
}

#[iced_cache]
pub struct Chat {
    #[hash]
    header: Header,
    #[hash]
    timeline: ChatTimeline,
    input: ChatInput,
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

        let room_id_log = room_id.clone();
        let stream = stream::once(async move {
            tracing::debug!("Building timeline for room {}", room_id_log);
            let timeline = match builder.build().await {
                Ok(t) => Arc::new(t),
                Err(e) => {
                    tracing::error!("Failed to build timeline for room {}: {:?}", room_id_log, e);
                    return stream::pending().left_stream(); // never resolves; room stays empty
                }
            };
            let (initial, updates) = timeline.subscribe().await;
            tracing::debug!(
                "Subscribed to timeline for room {} with {} initial items",
                room_id_log,
                initial.len()
            );

            if initial.is_empty() {
                match timeline.paginate_backwards(30).await {
                    Ok(reached_start) => tracing::debug!(
                        "Paginated room {} backwards (reached_start={})",
                        room_id_log,
                        reached_start
                    ),
                    Err(e) => {
                        tracing::warn!("Failed to paginate room {} backwards: {:?}", room_id_log, e)
                    }
                }
            }

            let initial = initial
                .into_iter()
                .map(|m| m.convert(&avatar_cache))
                .collect();

            stream::once(future::ready(TimelineMessage::Loaded { timeline, initial }))
                .chain(updates.map(TimelineMessage::Diffs))
                .right_stream()
        })
        .flatten();

        (
            Self {
                header: Header::new(state, &room),
                timeline: ChatTimeline::new(&room, &state),
                input: ChatInput::new(&room),
            },
            Task::stream(stream).map(move |msg| (room_id.clone(), msg)),
        )
    }

    pub fn get_input_pointer(&self) -> *mut text_editor::Content {
        self.input.get_raw_content_pointer()
    }
}

impl IcedWidget<ChatMessage, ChatAction> for Chat {
    fn update(&mut self, msg: ChatMessage) -> ChatAction {
        if let ChatMessage::Timeline(TimelineMessage::Loaded { timeline, .. }) = &msg {
            self.input.timeline = Some(timeline.clone())
        }

        match msg {
            ChatMessage::Header(msg) => match self.header.update(msg) {
                HeaderAction::None => ChatAction::None,
                HeaderAction::Run(task) => ChatAction::Run(task),
            },
            ChatMessage::Timeline(msg) => match self.timeline.update(msg) {
                TimelineAction::None => ChatAction::None,
                TimelineAction::SetReplying(event_id) => {
                    self.input.set_replies_to(event_id);
                    ChatAction::None
                }
            },
            ChatMessage::Input(msg) => match self.input.update(msg) {
                InputAction::None => ChatAction::None,
                InputAction::Run(task) => ChatAction::Run(task),
            },
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
                    top: 0.0,
                    left: structure.small_gap,
                    right: structure.small_gap,
                    bottom: structure.small_gap
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
