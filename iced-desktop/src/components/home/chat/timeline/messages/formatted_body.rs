//! Renders a [`FormattedBody`] (the parsed `formatted_body` HTML of a message, see
//! `deplace_core::rich_text`) as a column of blocks, following the layout conventions suggested
//! by the Matrix specification.

use deplace_core::rich_text::{Block, FormattedBody, Inline, MessageLink};
use iced::widget::text::Span;

use crate::common::*;

use super::TimelineItemMessage;

pub trait FormattedBodyView {
    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        is_local_echo: bool,
    ) -> Element<'static, TimelineItemMessage>;
}

impl FormattedBodyView for FormattedBody {
    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        is_local_echo: bool,
    ) -> Element<'static, TimelineItemMessage> {
        render_blocks(&self.blocks, theme, structure, is_local_echo, true)
    }
}

fn render_blocks(
    blocks: &[Block],
    theme: Theme,
    structure: Structure,
    is_local_echo: bool,
    is_first: bool,
) -> Element<'static, TimelineItemMessage> {
    w::Column::with_children(
        blocks.iter().enumerate().map(|(i, block)| {
            render_block(block, theme, structure, is_local_echo, i == 0 && is_first)
        }),
    )
    .spacing(structure.small_gap / 2.0)
    .width(Fill)
    .into()
}

fn render_block(
    block: &Block,
    theme: Theme,
    structure: Structure,
    is_local_echo: bool,
    is_first: bool,
) -> Element<'static, TimelineItemMessage> {
    let text_size = structure.chat.text_size;

    match block {
        Block::Paragraph(content) => render_inline(content, text_size, false, theme, is_local_echo),
        Block::Heading { level, content } => {
            let factor = heading_scale(*level);

            let mut padding = padding::bottom(structure.small_gap * factor);
            if !is_first {
                padding = padding.top(structure.small_gap * factor);
            }

            w::container(render_inline(
                content,
                text_size * factor,
                true,
                theme,
                is_local_echo,
            ))
            .padding(padding)
            .into()
        }
        Block::BlockQuote(blocks) => w::container(render_blocks(
            blocks,
            theme,
            structure,
            is_local_echo,
            false,
        ))
        .padding(structure.small_gap)
        .style(move |_| ContainerStyle {
            background: Some(theme.solid_bg.into()),
            border: Border {
                color: theme.border.into(),
                width: structure.border_thickness,
                radius: structure.smaller_border_radius.into(),
            },
            ..Default::default()
        })
        .width(Fill)
        .into(),
        Block::List {
            ordered,
            start,
            items,
        } => render_list(*ordered, *start, items, theme, structure, is_local_echo),
        Block::CodeBlock { code, .. } => w::container(
            w::scrollable(
                w::text(code.clone())
                    .font(Font::MONOSPACE)
                    .size(text_size * 0.9)
                    .color(theme.text.normal),
            )
            .direction(w::scrollable::Direction::Horizontal(
                w::scrollable::Scrollbar::default(),
            )),
        )
        .padding(structure.small_gap)
        .width(Fill)
        .style(move |_| ContainerStyle {
            background: Some(theme.backdrop.into()),
            border: border::rounded(structure.smaller_border_radius),
            ..Default::default()
        })
        .into(),
        Block::ThematicBreak => w::rule::horizontal(structure.border_thickness).into(),
    }
}

fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.5,
        2 => 1.25,
        3 => 1.125,
        _ => 1.0,
    }
}

fn render_list(
    ordered: bool,
    start: Option<i64>,
    items: &[Vec<Block>],
    theme: Theme,
    structure: Structure,
    is_local_echo: bool,
) -> Element<'static, TimelineItemMessage> {
    let text_size = structure.chat.text_size;
    let start = start.unwrap_or(1);

    w::Column::with_children(items.iter().enumerate().map(|(i, item_blocks)| {
        let marker: Element<'static, TimelineItemMessage> = if ordered {
            w::text(format!("{}.", start + i as i64))
                .size(text_size)
                .color(theme.text.normal)
                .into()
        } else {
            w::text("•").size(text_size).color(theme.text.normal).into()
        };

        w::row![
            marker,
            render_blocks(item_blocks, theme, structure, is_local_echo, false)
        ]
        .spacing(structure.small_gap / 2.0)
        .into()
    }))
    .spacing(structure.small_gap / 2.0)
    .padding(padding::left(structure.small_gap))
    .width(Fill)
    .into()
}

fn render_inline(
    content: &[Inline],
    text_size: f32,
    bold: bool,
    theme: Theme,
    is_local_echo: bool,
) -> Element<'static, TimelineItemMessage> {
    let default_color = if is_local_echo {
        theme.text.dim
    } else {
        theme.text.normal
    };

    let spans: Vec<Span<'static, MessageLink>> = content
        .iter()
        .map(|inline| render_inline_span(inline, bold, default_color, theme))
        .collect();

    w::rich_text(spans)
        .size(text_size)
        .width(Fill)
        .on_link_click(TimelineItemMessage::LinkClick)
        .into()
}

fn is_mention(link: &MessageLink) -> bool {
    !matches!(link, MessageLink::Url(_))
}

fn render_inline_span(
    inline: &Inline,
    force_bold: bool,
    default_color: DePlaceColor,
    theme: Theme,
) -> Span<'static, MessageLink> {
    let run = match inline {
        Inline::LineBreak => return w::span("\n".to_string()),
        Inline::Text(run) => run,
    };

    let mention = run.link.as_ref().is_some_and(is_mention);

    let mut color = run.color.unwrap_or(default_color);
    let mut background = run.background;

    if run.link.is_some() && run.color.is_none() {
        color = if mention {
            theme.pill_color
        } else {
            theme.accent
        };
    }
    if mention && run.background.is_none() {
        background = Some(theme.pill_color.scale_alpha(0.18));
    }

    if run.spoiler.is_some() {
        // No reveal-on-click yet: just mask the text so it reads like a spoiler bar.
        color = theme.text.dim;
        background = Some(theme.text.dim);
    }

    let bold = run.bold || force_bold;
    let font = (bold || run.italic || run.code).then_some(Font {
        weight: if bold { Weight::Bold } else { Weight::Normal },
        style: if run.italic {
            iced::font::Style::Italic
        } else {
            iced::font::Style::Normal
        },
        ..if run.code {
            Font::MONOSPACE
        } else {
            Font::DEFAULT
        }
    });

    w::span(run.text.clone())
        .font_maybe(font)
        .color(color.to_iced())
        .background_maybe(background.map(|c| c.to_iced()))
        .underline(run.underline || run.link.is_some())
        .strikethrough(run.strikethrough)
        .link_maybe(run.link.clone())
        .padding(if mention {
            padding::horizontal(4.0)
        } else {
            Padding::default()
        })
        .border_maybe(mention.then(|| border::rounded(6.0)))
}
