//! Renders a [`FormattedBody`] (the parsed `formatted_body` HTML of a message, see
//! `deplace_core::rich_text`) as a column of blocks, following the layout conventions suggested
//! by the Matrix specification.

use deplace_core::rich_text::{Block, FormattedBody, Inline, Mention};
use iced::widget::text::Span;

use crate::common::*;

use super::TimelineItemMessage;

pub trait FormattedBodyView {
    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        is_local_echo: bool,
        membership_map: &IndexMap<OwnedUserId, RoomMember>,
    ) -> Element<'static, TimelineItemMessage>;
}

impl FormattedBodyView for FormattedBody {
    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        is_local_echo: bool,
        membership_map: &IndexMap<OwnedUserId, RoomMember>,
    ) -> Element<'static, TimelineItemMessage> {
        render_blocks(
            &self.blocks,
            theme,
            structure,
            is_local_echo,
            true,
            membership_map,
        )
    }
}

fn render_blocks(
    blocks: &[Block],
    theme: Theme,
    structure: Structure,
    is_local_echo: bool,
    is_first: bool,
    membership_map: &IndexMap<OwnedUserId, RoomMember>,
) -> Element<'static, TimelineItemMessage> {
    w::Column::with_children(blocks.iter().enumerate().map(|(i, block)| {
        render_block(
            block,
            theme,
            structure,
            is_local_echo,
            i == 0 && is_first,
            membership_map,
        )
    }))
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
    membership_map: &IndexMap<OwnedUserId, RoomMember>,
) -> Element<'static, TimelineItemMessage> {
    let text_size = structure.chat.text_size;

    match block {
        Block::Paragraph(content) => render_inline(
            content,
            text_size,
            false,
            theme,
            structure,
            is_local_echo,
            membership_map,
        ),
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
                structure,
                is_local_echo,
                membership_map,
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
            membership_map,
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
        } => render_list(
            *ordered,
            *start,
            items,
            theme,
            structure,
            is_local_echo,
            membership_map,
        ),
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
    membership_map: &IndexMap<OwnedUserId, RoomMember>,
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
            render_blocks(
                item_blocks,
                theme,
                structure,
                is_local_echo,
                false,
                membership_map
            )
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
    structure: Structure,
    is_local_echo: bool,
    membership_map: &IndexMap<OwnedUserId, RoomMember>,
) -> Element<'static, TimelineItemMessage> {
    let default_color = if is_local_echo {
        theme.text.dim
    } else {
        theme.text.normal
    };

    let spans: Vec<Span<'static, TimelineItemMessage>> = content
        .iter()
        .map(|inline| {
            render_inline_span(
                inline,
                bold,
                default_color,
                theme,
                structure,
                membership_map,
            )
        })
        .collect();

    w::rich_text(spans)
        .size(text_size)
        .width(Fill)
        .on_link_click(|v| v)
        .into()
}

fn render_inline_span(
    inline: &Inline,
    force_bold: bool,
    default_color: DePlaceColor,
    theme: Theme,
    structure: Structure,
    membership_map: &IndexMap<OwnedUserId, RoomMember>,
) -> Span<'static, TimelineItemMessage> {
    let run = match inline {
        Inline::LineBreak => return w::span("\n".to_string()),
        Inline::Text(run) => run,
        Inline::Mention(mention) => {
            return render_mention(theme, structure, mention, membership_map);
        }
    };

    let mut color = run.color.unwrap_or(default_color);
    let mut background = run.background;

    if run.link.is_some() && run.color.is_none() {
        color = theme.accent;
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
        .underline(run.underline)
        .strikethrough(run.strikethrough)
        .link_maybe(
            run.link
                .as_ref()
                .map(|link| TimelineItemMessage::LinkClick(link.clone())),
        )
}

fn render_mention(
    theme: Theme,
    structure: Structure,
    mention: &Mention,
    membership_map: &IndexMap<OwnedUserId, RoomMember>,
) -> Span<'static, TimelineItemMessage> {
    let (text, color) = match mention {
        Mention::Event {
            room_or_alias_id,
            event_id,
        } => (
            format!("#{room_or_alias_id}/{event_id}"),
            theme.colors.yellow,
        ),
        Mention::Room(room_id) => (format!("#{room_id}"), room_id.as_str().into()),
        Mention::RoomAlias(alias) => (alias.to_string(), theme.colors.yellow),
        Mention::User {
            user_id,
            display_name,
        } => {
            let member = membership_map.get(user_id);

            let name = member
                .map(|m| m.get_name())
                .or(display_name.clone())
                .unwrap_or(user_id.to_string());
            let color = member.map(|m| m.color()).unwrap_or(user_id.as_str().into());

            (format!("@{name}"), color)
        }
    };

    w::span(text)
        .color(color)
        .padding(padding::horizontal(structure.divider_width))
        .border(border::rounded(structure.semi_border_radius()))
        .background(color.set_alpha(0.15))
}
