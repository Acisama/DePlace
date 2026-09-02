use macros::iced_cache;
use matrix_sdk_ui::{
    Timeline,
    eyeball_im::{Vector, VectorDiff},
    timeline::TimelineItem,
};

use crate::common::*;

#[derive(Debug, Clone)]
pub enum TimelineMessage {
    Loaded {
        timeline: Arc<Timeline>,
        initial: Vector<Arc<TimelineItem>>,
    },
    Diffs(Vec<VectorDiff<Arc<TimelineItem>>>),
    SetReplying(OwnedEventId),
}

pub enum TimelineAction {
    SetReplying(OwnedEventId),
    None,
}

#[iced_cache]
pub struct ChatTimeline {
    timeline: Option<Arc<Timeline>>,
    room: Room,

    messages: Vector<Arc<TimelineItem>>,
    #[hash]
    messages_version: u64,
}

impl ChatTimeline {
    pub fn new(room: &Room) -> Self {
        Self {
            timeline: None,
            room: room.clone(),

            messages: Vector::new(),
            messages_version: 0,
        }
    }
}

impl IcedWidget<TimelineMessage, TimelineAction> for ChatTimeline {
    fn update(&mut self, message: TimelineMessage) -> TimelineAction {
        match message {
            TimelineMessage::Loaded { timeline, initial } => {
                tracing::debug!(
                    "ChatTimeline for room {} loaded with {} initial items",
                    self.room.room_id(),
                    initial.len()
                );
                self.timeline = Some(timeline);
                self.messages = initial;
                self.messages_version += 1;
            }
            TimelineMessage::Diffs(diffs) => {
                tracing::debug!(
                    "ChatTimeline for room {} applying {} diff(s), {} messages before",
                    self.room.room_id(),
                    diffs.len(),
                    self.messages.len()
                );
                for diff in diffs {
                    diff.apply(&mut self.messages);
                }
                self.messages_version += 1;
            }
            TimelineMessage::SetReplying(event_id) => return TimelineAction::SetReplying(event_id),
        };

        TimelineAction::None
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, TimelineMessage> {
        w::container(
            Column::with_children(
                self.messages
                    .iter()
                    .map(|c| w::text(format!("{:?}", c)).into()),
            )
            .width(Fill),
        )
        .width(Fill)
        .align_bottom(Fill)
        .into()
    }
}
