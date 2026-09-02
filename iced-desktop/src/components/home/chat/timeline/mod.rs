use std::hash::Hasher;

use macros::iced_cache;
use matrix_sdk_ui::{
    Timeline,
    eyeball_im::{Vector, VectorDiff},
    timeline::TimelineItem as UiTimelineItem,
};
use messages::TimelineItem;

use crate::common::*;

mod messages;
pub use messages::ToTimelineItem;

#[derive(Debug, Clone)]
pub enum TimelineMessage {
    Loaded {
        timeline: Arc<Timeline>,
        initial: Vector<Arc<TimelineItem>>,
    },
    Diffs(Vec<VectorDiff<Arc<UiTimelineItem>>>),
    SetReplying(OwnedEventId),
}

pub enum TimelineAction {
    SetReplying(OwnedEventId),
    None,
}

#[iced_cache]
pub struct ChatTimeline {
    timeline: Option<Arc<Timeline>>,

    #[ignore]
    avatar_cache: AvatarCache,

    #[hash]
    room_id: OwnedRoomId,

    messages: Vector<Arc<TimelineItem>>,
    #[hash]
    messages_version: u64,
}

impl ChatTimeline {
    pub fn new(room: &Room, state: &AppState) -> Self {
        Self {
            timeline: None,
            room_id: room.room_id().to_owned(),

            avatar_cache: state.avatar_cache().clone(),

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
                    self.room_id,
                    initial.len()
                );
                self.timeline = Some(timeline);
                self.messages = initial;
                self.messages_version += 1;
            }
            TimelineMessage::Diffs(diffs) => {
                tracing::debug!(
                    "ChatTimeline for room {} applying {} diff(s), {} messages before",
                    self.room_id,
                    diffs.len(),
                    self.messages.len()
                );

                for diff in diffs
                    .into_iter()
                    .map(|d| d.map(|m| m.convert(&self.avatar_cache)))
                {
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
            w::scrollable(
                w::keyed_column(self.messages.iter().map(|item| {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    item.id.hash(&mut hasher);
                    let key = hasher.finish();

                    let item = item.clone();
                    let element = w::lazy(item.clone(), move |item| item.view(theme, structure));
                    (key, element.into())
                }))
                .width(Fill),
            )
            .anchor_bottom()
            .width(Fill),
        )
        .width(Fill)
        .align_bottom(Fill)
        .into()
    }
}
