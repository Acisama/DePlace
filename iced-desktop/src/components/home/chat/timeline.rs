use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::common::*;

#[derive(Debug, Clone)]
pub enum TimelineMessage {
    SetReplying(OwnedEventId),
}

pub enum TimelineAction {
    Run(Task<()>),
    SetReplying(OwnedEventId),
    None,
}

#[iced_cache]
pub struct ChatTimeline {
    pub timeline: Option<Arc<Timeline>>,
    room: Room,

    #[hash]
    messages: Vec<()>,
}

impl ChatTimeline {
    pub fn new(room: &Room) -> Self {
        Self {
            timeline: None,
            room: room.clone(),
            messages: Vec::new(),
        }
    }
}

impl IcedWidget<TimelineMessage, TimelineAction> for ChatTimeline {
    fn update(&mut self, message: TimelineMessage) -> TimelineAction {
        match message {
            TimelineMessage::SetReplying(event_id) => TimelineAction::SetReplying(event_id),
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, TimelineMessage> {
        w::container("test").width(Fill).height(Fill).into()
    }
}
