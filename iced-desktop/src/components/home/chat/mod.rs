use header::{Header, HeaderAction, HeaderMessage};
use input::{ChatInput, InputAction, InputMessage};
use macros::iced_cache;
use matrix_sdk_ui::{
    Timeline,
    timeline::{DateDividerMode, TimelineBuilder, TimelineFocus, TimelineReadReceiptTracking},
};
use timeline::{ChatTimeline, TimelineAction, TimelineMessage};

use crate::common::*;
pub(super) mod empty;
mod header;
mod input;
mod timeline;

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
    pub fn new(state: &AppState, room: Room) -> (Self, Task<Option<(OwnedRoomId, Timeline)>>) {
        let builder = TimelineBuilder::new(&room)
            .with_date_divider_mode(DateDividerMode::Daily)
            .with_focus(TimelineFocus::Live {
                hide_threaded_events: false,
            })
            .track_read_marker_and_receipts(TimelineReadReceiptTracking::AllEvents)
            .add_failed_to_parse(true);

        let room_id = room.room_id().to_owned();

        (
            Self {
                header: Header::new(state, &room),
                timeline: ChatTimeline::new(&room),
                input: ChatInput::new(&room),
            },
            Task::future(async move {
                match builder.build().await {
                    Ok(timeline) => Some((room_id, timeline)),
                    Err(e) => {
                        tracing::error!("Failed to build timeline: {:?}", e);
                        None
                    }
                }
            }),
        )
    }

    pub fn insert_timeline(&mut self, timeline: Arc<Timeline>) {
        self.timeline.timeline = Some(timeline.clone());
        self.input.timeline = Some(timeline);
    }
}

impl IcedWidget<ChatMessage, ChatAction> for Chat {
    fn update(&mut self, msg: ChatMessage) -> ChatAction {
        match msg {
            ChatMessage::Header(msg) => match self.header.update(msg) {
                HeaderAction::None => ChatAction::None,
                HeaderAction::Run(task) => ChatAction::Run(task),
            },
            ChatMessage::Timeline(msg) => match self.timeline.update(msg) {
                TimelineAction::None => ChatAction::None,
                TimelineAction::Run(task) => ChatAction::Run(task),
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
