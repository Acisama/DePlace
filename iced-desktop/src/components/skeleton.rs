use iced::{Alignment, Length};

use crate::common::*;
use crate::components::PushElement;

/// One line of a placeholder message's text block: either a row of
/// word-shaped bars (widths are fractions of the line's available width),
/// or a standalone inline media placeholder taking up its own line.
enum LineEntry {
    Words(&'static [f32]),
    Media,
}

/// One placeholder message: `name_length` is the name bar's width as a
/// fraction of the row's available width; `lines` is the text block below
/// it, one entry per line, so a message can span multiple wrapped text
/// lines and/or inline media.
struct SkeletonMessage {
    name_length: f32,
    lines: &'static [LineEntry],
}

const MESSAGES: &[SkeletonMessage] = &[
    SkeletonMessage {
        name_length: 0.16,
        lines: &[
            LineEntry::Words(&[0.16, 0.07, 0.10]),
            LineEntry::Words(&[0.14, 0.08, 0.10]),
            LineEntry::Words(&[0.09, 0.07, 0.12, 0.08]),
            LineEntry::Words(&[0.11, 0.06, 0.09]),
        ],
    },
    SkeletonMessage {
        name_length: 0.13,
        lines: &[LineEntry::Words(&[0.09, 0.11, 0.07]), LineEntry::Media],
    },
    SkeletonMessage {
        name_length: 0.12,
        lines: &[
            LineEntry::Words(&[0.09, 0.14, 0.07, 0.10, 0.06]),
            LineEntry::Words(&[0.08, 0.12, 0.07]),
        ],
    },
    SkeletonMessage {
        name_length: 0.18,
        lines: &[
            LineEntry::Words(&[0.10, 0.08, 0.13, 0.07]),
            LineEntry::Words(&[0.09, 0.15, 0.08]),
            LineEntry::Words(&[0.11, 0.07, 0.14, 0.09]),
            LineEntry::Words(&[0.08, 0.10, 0.07]),
        ],
    },
    SkeletonMessage {
        name_length: 0.10,
        lines: &[LineEntry::Words(&[0.09, 0.12, 0.07])],
    },
    SkeletonMessage {
        name_length: 0.14,
        lines: &[
            LineEntry::Words(&[0.08, 0.16, 0.09, 0.11]),
            LineEntry::Words(&[0.10, 0.07, 0.13]),
            LineEntry::Words(&[0.09, 0.15, 0.08, 0.06]),
            LineEntry::Words(&[0.07, 0.11]),
        ],
    },
    SkeletonMessage {
        name_length: 0.11,
        lines: &[LineEntry::Words(&[0.08, 0.10, 0.06]), LineEntry::Media],
    },
];

/// How far a single bar's gray tone can drift from the base color, so the
/// block reads as a plausible mix of shades rather than a flat wall of
/// identical blocks.
const TONE_VARIANCE: f32 = 0.12;

/// A solid bar/circle that breathes in and out via [`animated_background`],
/// rather than snapping between two fixed colors.
fn pulsing_block<'a, Message: 'a>(
    width: impl Into<Length>,
    height: f32,
    radius: f32,
    color: DePlaceColor,
) -> Element<'a, Message> {
    animated_background(EMPTY_WIDGET)
        .width(width)
        .height(height)
        .style(move |pulse| ContainerStyle {
            background: Some(color.scale_alpha(0.3 + pulse * 0.35).into()),
            border: border::rounded(radius),
            ..Default::default()
        })
        .boxed()
}

/// An inline media placeholder: a fixed-size (not width-fractional) square,
/// since media doesn't scale with the surrounding text the way a word does.
fn media_block<'a, Message: 'a>(
    size: f32,
    rounded: f32,
    color: DePlaceColor,
) -> Element<'a, Message> {
    pulsing_block(size, size, rounded, color)
}

/// A deterministic pseudo-random value in `-1.0..=1.0` for `seed` -- the
/// same `seed` always maps to the same value, so a bar's tone offset looks
/// "random" without actually varying between renders or needing to be
/// stored anywhere.
fn pseudo_random(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E3779B9) ^ 0x85EBCA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B3C6D);
    x ^= x >> 12;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// `color`, nudged a bit lighter or darker based on `seed`.
fn toned(color: DePlaceColor, seed: u32) -> DePlaceColor {
    let [_, _, lightness, _] = color.to_hsla();
    color.set_lightness((lightness + pseudo_random(seed) * TONE_VARIANCE).clamp(0.0, 1.0))
}

/// A bar occupying `fraction` of its row's width, the rest left empty --
/// `Length` alone can't express "55% of whatever space I'm given", so a
/// trailing spacer with the complementary [`Length::FillPortion`] soaks up
/// the remainder instead.
fn fractional_bar<'a, Message: 'a>(
    height: f32,
    fraction: f32,
    color: DePlaceColor,
) -> Element<'a, Message> {
    let portion = ((fraction * 100.0).round() as u16).clamp(1, 99);

    w::row![
        pulsing_block(Length::FillPortion(portion), height, height / 2.0, color),
        Space::new().width(Length::FillPortion(100 - portion)),
    ]
    .boxed()
}

/// A placeholder text line: a row of word bars, plus a trailing spacer
/// absorbing whatever width the words don't use. `seed` offsets each
/// word's tone so they don't all land on the same pseudo-random value.
fn word_row<'a, Message: 'a>(
    height: f32,
    gap: f32,
    words: &'static [f32],
    color: DePlaceColor,
    seed: u32,
) -> Element<'a, Message> {
    let mut row = w::Row::new();

    for (word_index, &fraction) in words.iter().enumerate() {
        let portion = ((fraction * 100.0).round() as u16).max(1);
        row = row.push_element(pulsing_block(
            Length::FillPortion(portion),
            height,
            height / 2.0,
            toned(color, seed + word_index as u32),
        ));
    }

    let used: f32 = words.iter().sum();
    let remaining = ((1.0 - used).max(0.0) * 100.0).round() as u16;
    row = row.push_element(Space::new().width(Length::FillPortion(remaining.max(1))));

    row.spacing(gap).boxed()
}

/// The full placeholder text block below a name bar: one row per line,
/// either a [`word_row`] or a standalone [`media_block`], stacked
/// vertically.
fn text_block<'a, Message: 'a>(
    height: f32,
    gap: f32,
    media_size: f32,
    rounded: f32,
    lines: &'static [LineEntry],
    color: DePlaceColor,
    seed: u32,
) -> Element<'a, Message> {
    let mut column = w::Column::new();

    for (line_index, line) in lines.iter().enumerate() {
        let line_seed = seed + line_index as u32 * 100;
        column = column.push_element(match line {
            LineEntry::Words(words) => word_row(height, gap, words, color, line_seed),
            LineEntry::Media => media_block(media_size, rounded, toned(color, line_seed)),
        });
    }

    column.spacing(gap * 2.0).boxed()
}

/// A fixed-height block of placeholder message rows (avatar + name/word
/// bars) with a pulsing shimmer, meant to sit above or below the loaded
/// timeline items while more history is still being paginated in.
pub fn skeleton<'a, Message: 'a>(
    structure: Structure,
    color: DePlaceColor,
) -> Element<'a, Message> {
    let icon_size = structure.chat.icon_size;
    let gap = structure.small_gap;
    let bar_height = structure.chat.text_size;

    w::column(MESSAGES.iter().enumerate().map(|(message_index, message)| {
        // Spread seeds out per message so neighboring messages/words don't
        // land on correlated tone offsets.
        let seed = message_index as u32 * 1000;

        w::row![
            pulsing_block(icon_size, icon_size, icon_size / 2.0, toned(color, seed)),
            w::column![
                fractional_bar(bar_height, message.name_length, toned(color, seed + 1)),
                text_block(
                    bar_height,
                    gap / 2.0,
                    structure.chat.max_media_height / 2.0,
                    structure.inner_border_radius,
                    message.lines,
                    color,
                    seed + 2
                ),
            ]
            .spacing(structure.gap)
            .width(Fill),
        ]
        .spacing(structure.gap)
        .align_y(Alignment::Start)
        .boxed()
    }))
    .spacing(gap * 3.0)
    .padding(padding::left(structure.small_gap * 2.0))
    .boxed()
}
