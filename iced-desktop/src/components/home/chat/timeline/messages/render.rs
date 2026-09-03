use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineDetails};

use crate::{common::*, components::home::chat::TimelineMessage};

use super::{MessageEvent, TimelineItem, TimelineItemKind, TimelineItemMessage};

impl MessageEvent {
    pub fn view(
        &self,
        theme: Theme,
        structure: Structure,
        avatar_cache: &AvatarCache,
        thumbnail_cache: &ThumbnailCache,
    ) -> iced::Element<'static, TimelineItemMessage> {
        let col_width = structure.chat_col_width();
        let pre_col_width = structure.small_gap * 1.5;

        let text_size = structure.chat.text_size;

        let show_header = true;

        let render_text = |text: String| w::text(text).color(theme.text.normal).size(text_size);

        let (text_content, other_content) = match &self.content {
            MsgLikeKind::Message(msg) => match msg.msgtype() {
                MessageType::Text(text) => (
                    Some(render_text(text.body.clone())),
                    None::<iced::Element<'static, TimelineItemMessage>>,
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

        let (icon, name) = if show_header {
            {
                let size = structure.chat.icon_size;
                let rounding = size / 2.0;

                match &self.sender_profile {
                    TimelineDetails::Error(_) | TimelineDetails::Unavailable => (
                        Some(unknown_icon(size, rounding, theme)),
                        Some(render_unknown_name(size, theme)),
                    ),
                    TimelineDetails::Pending => (
                        Some(loading_icon(size, rounding, theme)),
                        Some(render_loading_name(size, theme)),
                    ),
                    TimelineDetails::Ready(p) => (
                        Some(p.render_icon(size, &avatar_cache)),
                        Some(p.render_name(text_size)),
                    ),
                }
            }
        } else {
            (None, None)
        };

        w::button(w::row![
            w::row![
                Space::new().width(pre_col_width),
                icon.unwrap_or(Space::new().into())
            ]
            .width(col_width),
            w::column![name.unwrap_or(Space::new().into()), column]
        ])
        .padding(structure.small_gap)
        .style(move |_, status| ButtonStyle {
            background: None,
            border: Border {
                color: if status.active() {
                    theme.border
                } else {
                    Color::TRANSPARENT
                },
                width: structure.border_thickness,
                radius: structure.semi_border_radius().into(),
            },
            ..Default::default()
        })
        .width(Fill)
        .on_press(TimelineItemMessage::None)
        .into()
    }
}
