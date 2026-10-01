use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::common::*;

#[derive(Clone, Debug)]
pub enum MessageMessage {
    SetReplyingTo,
    Delete,
    Pin,
    Edit,
}

#[derive(Clone, Debug)]
pub enum ContextMenuMessage {
    Message {
        timeline: Arc<Timeline>,
        room_id: OwnedRoomId,
        event_id: Option<OwnedEventId>,
        item_id: String,
        message: MessageMessage,
    },
}

pub enum ContextMenuAction {
    SetReplyingTo {
        item_id: String,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
    },
}

#[iced_cache(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    pub position: Point,
    #[hash]
    pub kind: ContextMenuKind,
}

#[derive(Clone, Debug)]
pub enum ContextMenuKind {
    Message {
        timeline: Arc<Timeline>,
        event_id: OwnedEventId,
        item_id: String,
    },
}

impl Hash for ContextMenuKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ContextMenuKind::Message { .. } => 0.hash(state),
        }
    }
}

impl PartialEq for ContextMenuKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                ContextMenuKind::Message { event_id, .. },
                ContextMenuKind::Message {
                    event_id: event_id2,
                    ..
                },
            ) => event_id == event_id2,
        }
    }
}

impl IcedWidget<ContextMenuMessage, ContextMenuAction> for ContextMenu {
    fn update(&mut self, message: ContextMenuMessage) -> Option<ContextMenuAction> {
        match message {
            ContextMenuMessage::Message {
                timeline,
                event_id,
                room_id,
                item_id,
                message,
            } => match message {
                MessageMessage::SetReplyingTo => {
                    let Some(event_id) = event_id else {
                        tracing::warn!("No event_id for SetReplyingTo action");
                        return None;
                    };

                    Some(ContextMenuAction::SetReplyingTo {
                        item_id,
                        room_id,
                        event_id,
                    })
                }
                // TODO
                MessageMessage::Delete => None,
                // TODO
                MessageMessage::Pin => None,
                // TODO
                MessageMessage::Edit => None,
            },
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        _help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, ContextMenuMessage> {
        let position = self.position;

        w::container(
            w::container(Space::new().width(100.0).height(100.0)).style(move |_| ContainerStyle {
                background: Some(theme.solid_bg.into()),
                border: Border {
                    color: theme.border.into(),
                    width: structure.border_thickness,
                    radius: structure.inner_border_radius.into(),
                },
                ..Default::default()
            }),
        )
        .width(Fill)
        .height(Fill)
        .padding(Padding {
            top: position.y,
            left: position.x,
            right: 0.0,
            bottom: 0.0,
        })
        .into()
    }
}
