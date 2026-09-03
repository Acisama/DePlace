use iced::never;
use matrix_sdk_ui::timeline::TimelineDetails;

use crate::{common::*, components::home::chat::TimelineMessage};

use super::{MessageContent, MessageEvent, TimelineItem, TimelineItemKind, TimelineItemMessage};

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

        let (text_content, other_content) = render_message_kind(&self.content, &theme, &structure);

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
                        Some(p.render_icon(size, avatar_cache)),
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
        .padding(padding::vertical(structure.small_gap))
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

fn render_message_kind(
    kind: &MessageContent,
    theme: &Theme,
    structure: &Structure,
) -> (
    Option<Element<'static, TimelineItemMessage>>,
    Option<Element<'static, TimelineItemMessage>>,
) {
    let text_size = structure.chat.text_size;

    let render_text_color = |text: String, color: Color| w::text(text).color(color).size(text_size);
    let render_normal_text = |text: String| render_text_color(text, theme.text.normal);
    let render_warning_text = |text: &'static str| {
        (
            Some(render_text_color(text.to_string(), theme.colors.warning).into()),
            None,
        )
    };
    let render_error_text = |text: &'static str| {
        (
            Some(render_text_color(text.to_string(), theme.colors.error).into()),
            None,
        )
    };
    let itallic_text = |text: String| {
        (
            Some(
                w::rich_text![w::span(text).font(Font {
                    style: iced::font::Style::Italic,
                    ..Default::default()
                })]
                .on_link_click(never)
                .size(text_size)
                .into(),
            ),
            None,
        )
    };

    match kind {
        MessageContent::Audio => render_warning_text("Audio messages are not yet implemented"),
        MessageContent::Emote {
            body,
            formatted_body,
        } => (
            body.as_ref().map(|t| render_normal_text(t.clone()).into()),
            None,
        ),
        MessageContent::Empty => itallic_text("Empty".to_string()),
        MessageContent::File { .. } => render_warning_text("File messages are not yet implemented"),
        MessageContent::Image { .. } => {
            render_warning_text("Image messages are not yet implemented")
        }
        MessageContent::LiveLocation => {
            render_warning_text("Live location messages are not yet implemented")
        }
        MessageContent::Location => {
            render_warning_text("Location messages are not yet implemented")
        }
        MessageContent::Notice { .. } => {
            render_warning_text("Notice messages are not yet implemented")
        }
        MessageContent::Other { event_type } => {
            itallic_text(format!("Message of type: {}", event_type))
        }
        MessageContent::ServerNotice { .. } => {
            render_warning_text("Server notice messages are not yet implemented")
        }
        MessageContent::Poll => render_warning_text("Poll messages are not yet implemented"),
        MessageContent::Redacted => itallic_text("Redacted".to_string()),
        MessageContent::Sticker => render_warning_text("Sticker messages are not yet implemented"),
        MessageContent::Text {
            body,
            formatted_body,
            ..
        } => (
            body.as_ref().map(|t| render_normal_text(t.clone()).into()),
            None,
        ),
        MessageContent::UnableToDecrypt => {
            render_error_text("Unable to decrypt messages are not yet implemented")
        }
        MessageContent::VerificationRequest {
            body,
            formatted_body,
        } => (
            body.as_ref().map(|t| render_normal_text(t.clone()).into()),
            None,
        ),
        MessageContent::Video { .. } => {
            render_warning_text("Video messages are not yet implemented")
        }
    }
}
