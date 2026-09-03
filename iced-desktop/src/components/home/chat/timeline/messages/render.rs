use deplace_core::formatting::{fit_dimensions, format_bytes};
use iced::never;
use matrix_sdk::media::UniqueKey;
use matrix_sdk_ui::timeline::TimelineDetails;

use crate::{common::*, components::InsetShadow};

use super::{MessageContent, MessageEvent, TimelineItemMessage};

impl MessageEvent {
    pub fn view(
        &self,
        theme: Theme,
        structure: Structure,
        avatar_cache: &AvatarCache,
        thumbnail_cache: &ThumbnailCache,
        is_hovered: bool,
    ) -> iced::Element<'static, TimelineItemMessage> {
        let col_width = structure.chat_col_width();
        let pre_col_width = structure.small_gap * 1.5;
        let text_size = structure.chat.text_size;

        let show_header = true;

        let (text_content, other_content) = render_message_kind(
            &self.content,
            theme,
            structure,
            thumbnail_cache,
            self.media_hovered,
        );

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

        w::mouse_area(
            w::container(w::row![
                w::row![
                    Space::new().width(pre_col_width),
                    icon.unwrap_or(Space::new().into())
                ]
                .width(col_width),
                w::column![name.unwrap_or(Space::new().into()), column]
            ])
            .padding(padding::vertical(structure.small_gap))
            .style(move |_| ContainerStyle {
                background: None,
                border: Border {
                    color: if is_hovered {
                        theme.border
                    } else {
                        Color::TRANSPARENT
                    },
                    width: structure.border_thickness,
                    radius: structure.semi_border_radius().into(),
                },
                ..Default::default()
            })
            .width(Fill),
        )
        .on_enter(TimelineItemMessage::MessageEnter)
        .on_exit(TimelineItemMessage::MessageExit)
        .into()
    }
}

fn render_message_kind(
    kind: &MessageContent,
    theme: Theme,
    structure: Structure,
    thumbnail_cache: &ThumbnailCache,
    media_hovered: bool,
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
        MessageContent::Image {
            caption,
            formatted_caption,
            blur_preview,
            filename,
            source,
            info,
        } => {
            let max_width = structure.chat.max_media_width;
            let max_height = structure.chat.max_media_height;

            let label = format!(
                "{filename}{}",
                info.as_ref()
                    .and_then(|i| i.size.map(|s| format!(
                        " ({})",
                        format_bytes(s, deplace_core::settings::DataSizeUnit::Bytes)
                    )))
                    .unwrap_or_default()
            );

            // Just an overestimate to be sure
            let min_width = label.len() as f32 * structure.chat.text_size;

            let (width, height) = fit_dimensions(
                info.as_ref()
                    .and_then(|i| i.width.map(|w| w as f32))
                    .unwrap_or(max_width),
                info.as_ref()
                    .and_then(|i| i.height.map(|h| h as f32))
                    .unwrap_or(max_height),
                max_width,
                max_height,
                min_width,
            );

            let thumbnail_source = info
                .as_ref()
                .and_then(|i| i.thumbnail_source.clone())
                .unwrap_or_else(|| source.clone());

            let thumbnail_key = (thumbnail_source.unique_key(), width as u64, height as u64);
            let cached_image = thumbnail_cache.get(&thumbnail_key);
            let image = cached_image.clone().unwrap_or_default();

            let mut stack = Stack::new();

            if let Some(image) = blur_preview {
                stack = stack.push(
                    w::image(image)
                        .width(width)
                        .height(height)
                        .content_fit(iced::ContentFit::Fill)
                        .border_radius(structure.inner_border_radius),
                );
            }

            stack = match image {
                MediaState::Failed => stack
                    .push(
                        w::container(
                            weighted_text("Image failed to load", Weight::Bold)
                                .size(structure.chat.text_size * 1.5),
                        )
                        .width(Fill)
                        .height(Fill)
                        .center(Fill)
                        .style(move |_| w::container::Style {
                            background: Some(theme.colors.error.scale_lightness(0.2).into()),
                            text_color: Some(theme.colors.error),
                            border: Border {
                                color: theme.colors.error,
                                width: 0.0,
                                radius: 0.0.into(),
                            },
                            ..Default::default()
                        }),
                    )
                    .push(
                        Canvas::new(InsetShadow::new(
                            structure.inner_border_radius,
                            theme.colors.error,
                            structure.chat.text_size / 2.0,
                            8,
                        ))
                        .width(width)
                        .height(height),
                    ),
                MediaState::Loaded(image) => stack.push(
                    w::image((*image).clone())
                        .width(width)
                        .height(height)
                        .border_radius(structure.inner_border_radius),
                ),
                _ => stack,
            };

            if media_hovered {
                stack = stack.push(
                    w::container(
                        w::container(w::text(label).size(text_size).color(theme.text.normal))
                            .style(move |_| ContainerStyle {
                                background: Some(theme.solid_bg.into()),
                                border: Border {
                                    color: theme.border,
                                    width: structure.border_thickness,
                                    radius: ((structure.smaller_border_radius
                                        + structure.inner_border_radius)
                                        / 2.0)
                                        .into(),
                                },
                                ..Default::default()
                            })
                            .padding(structure.small_gap / 2.0),
                    )
                    .padding(structure.small_gap / 2.0)
                    .align_bottom(height),
                )
            }

            let media = w::mouse_area(w::container(stack).width(width).height(height).style(
                move |_| ContainerStyle {
                    border: Border {
                        color: Color::TRANSPARENT,
                        width: 0.0,
                        radius: structure.inner_border_radius.into(),
                    },
                    ..Default::default()
                },
            ));

            let media: Element<'static, TimelineItemMessage> = if cached_image.is_none() {
                on_appear(
                    media,
                    TimelineItemMessage::NeedsThumbnail {
                        source: thumbnail_source,
                        width: width as u64,
                        height: height as u64,
                    },
                )
                .into()
            } else {
                media.into()
            };

            (
                caption
                    .as_ref()
                    .map(|c| render_normal_text(c.clone()).into()),
                Some(
                    w::mouse_area(media)
                        .on_enter(TimelineItemMessage::MediaMouseEnter)
                        .on_exit(TimelineItemMessage::MediaMouseLeave)
                        .interaction(Interaction::Pointer)
                        .into(),
                ),
            )
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
