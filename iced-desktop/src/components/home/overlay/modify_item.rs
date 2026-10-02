use iced::{Length, widget::space};
use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::common::*;

#[derive(Clone, Debug)]
pub enum ModifyItemMessage {
    Delete { reason: String },
    Pin,
    Unpin,
    Close,
}

pub enum ModifyItemAction {
    Run(Task<()>),
    Close,
}

#[derive(Clone, Copy, Hash, Debug, PartialEq)]
enum ModifyItemKind {
    Delete,
    Pin,
    Unpin,
}

#[iced_cache(Clone, Debug)]
pub struct ModifyItem {
    timeline: Arc<Timeline>,
    #[hash]
    event_id: OwnedEventId,
    #[hash]
    kind: ModifyItemKind,
}

impl PartialEq for ModifyItem {
    fn eq(&self, other: &Self) -> bool {
        self.event_id == other.event_id && self.kind == other.kind
    }
}

impl ModifyItem {
    pub fn delete(timeline: Arc<Timeline>, event_id: OwnedEventId) -> Self {
        Self {
            timeline,
            event_id,
            kind: ModifyItemKind::Delete,
        }
    }

    pub fn pin(timeline: Arc<Timeline>, event_id: OwnedEventId, is_pinned: bool) -> Self {
        Self {
            timeline,
            event_id,
            kind: if is_pinned {
                ModifyItemKind::Unpin
            } else {
                ModifyItemKind::Pin
            },
        }
    }
}

impl IcedWidget<ModifyItemMessage, ModifyItemAction> for ModifyItem {
    fn update(&mut self, message: ModifyItemMessage) -> Option<ModifyItemAction> {
        match message {
            ModifyItemMessage::Close => Some(ModifyItemAction::Close),
            ModifyItemMessage::Delete { reason } => {
                let timeline = self.timeline.clone();
                let event_id = self.event_id.clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = timeline
                        .redact(
                            &matrix_sdk_ui::timeline::TimelineEventItemId::EventId(event_id),
                            Some(&reason),
                        )
                        .await
                    {
                        tracing::error!("Failed to redact event: {}", e);
                    }
                })))
            }
            ModifyItemMessage::Pin => {
                let room = self.timeline.room().clone();
                let event_id = self.event_id.clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = room.pin_event(&event_id).await {
                        tracing::error!("Failed to pin event: {}", e);
                    }
                })))
            }
            ModifyItemMessage::Unpin => {
                let room = self.timeline.room().clone();
                let event_id = self.event_id.clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = room.unpin_event(&event_id).await {
                        tracing::error!("Failed to unpin event: {}", e);
                    }
                })))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, ModifyItemMessage> {
        floating_tile(theme, structure, w::container("test")).into()
    }
}
