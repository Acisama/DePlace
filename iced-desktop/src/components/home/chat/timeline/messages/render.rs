use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk_ui::timeline::MsgLikeKind;
use tracing_subscriber::fmt::format;

use crate::{common::*, components::home::chat::TimelineMessage};

use super::{MessageEvent, TimelineItem, TimelineItemKind};

impl TimelineItem {
    pub fn view(
        &self,
        theme: Theme,
        structure: Structure,
    ) -> iced::Element<'static, TimelineMessage> {
        let fallback = w::text(format!("{:?}", self)).into();

        match &self.kind {
            TimelineItemKind::DateDivider(_) => fallback,
            TimelineItemKind::FailedToParseMessageLike { .. } => fallback,
            TimelineItemKind::FailedToParseState { .. } => fallback,
            TimelineItemKind::ReadMarker => fallback,
            TimelineItemKind::TimelineStart => fallback,
            TimelineItemKind::System(_) => fallback,
            TimelineItemKind::Message(msg) => msg.view(theme, structure),
        }
    }
}

impl MessageEvent {
    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, TimelineMessage> {
        let col_width = structure.chat_col_width();

        let (text_content, other_content) = match &self.content {
            MsgLikeKind::Message(msg) => match msg.msgtype() {
                MessageType::Text(text) => (
                    Some(w::text(text.body.clone())),
                    None::<iced::Element<'static, TimelineMessage>>,
                ),
                _ => (None, None),
            },
            _ => (None, None),
        };

        let mut column = w::Column::new();

        if let Some(text_content) = text_content {
            column = column.push(text_content);
        }
        if let Some(other_content) = other_content {
            column = column.push(other_content);
        }

        column.into()
    }
}
