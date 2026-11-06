use deplace_core::room::call::CallMember;
use iced::{Alignment, Length, border::Radius};

use crate::{
    common::*,
    components::{PushElement, volume_bar::Direction},
};

use super::ChatMessage;

const MAX_COLUMNS: usize = 4;

pub fn render_call_members(
    members: Vec<CallMember>,
    theme: Theme,
    structure: Structure,
    direction: Direction,
) -> Element<'static, ChatMessage> {
    let icon_size = structure.chat.icon_size * 2.0;
    let height = icon_size * 2.5;
    let rounding = icon_size / 4.0;

    let mut column = w::Column::new()
        .width(Fill)
        .padding(icon_size / 2.0)
        .spacing(icon_size / 2.0);

    let total_length = members.len();

    let gap = structure.icon_gap_ratio * icon_size / 4.0;

    for chunk in members.chunks(MAX_COLUMNS) {
        let mut row = w::Row::new().spacing(icon_size / 2.0);
        let fillers = MAX_COLUMNS - chunk.len();

        if fillers > 0 && total_length >= MAX_COLUMNS {
            row = row.push_element(
                Space::new()
                    .width(Length::FillPortion(fillers as u16))
                    .height(height),
            );
        }

        for member in chunk {
            let color = member.color();
            let is_speaking = member.is_speaking();

            row = row.push_element(
                w::container(
                    w::container(member.render_icon(icon_size))
                        .colored_rounded(color.to_iced().scale_lightness(0.15), rounding)
                        .width(Length::FillPortion(2))
                        .height(height)
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center),
                )
                .style(move |_| {
                    ContainerStyle::default().border(Border {
                        color: if is_speaking {
                            theme.colors.success.into()
                        } else {
                            Color::TRANSPARENT
                        },
                        width: gap,
                        radius: (rounding + gap * 2.0).into(),
                    })
                })
                .padding(gap * 2.0),
            );
        }

        if fillers > 0 && total_length >= MAX_COLUMNS {
            row = row.push_element(
                Space::new()
                    .width(Length::FillPortion(fillers as u16))
                    .height(height),
            );
        }

        column = column.push(row);
    }

    let style = ContainerStyle {
        background: Some(theme.solid_bg.into()),
        border: border::rounded(match direction {
            Direction::Right => Radius::default().left(structure.outer_border_radius),
            Direction::Left => Radius::default().right(structure.outer_border_radius),
            Direction::Top => Radius::default().bottom(structure.outer_border_radius),
            Direction::Bottom => Radius::default().top(structure.outer_border_radius),
        }),
        ..Default::default()
    };
    let content = w::container(
        w::container(column)
            .width(Fill)
            .height(Fill)
            .align_y(Alignment::Center)
            .style(move |_| style),
    )
    .padding(structure.border_thickness)
    .width(Fill)
    .height(Fill);

    match direction {
        Direction::Right => {
            w::row![content, divider(theme, structure.border_thickness, Fill)].boxed()
        }
        Direction::Left => {
            w::row![divider(theme, structure.border_thickness, Fill), content].boxed()
        }
        Direction::Top => {
            w::column![divider(theme, Fill, structure.border_thickness), content].boxed()
        }
        Direction::Bottom => {
            w::column![content, divider(theme, Fill, structure.border_thickness)].boxed()
        }
    }
}

fn divider<'a>(
    theme: Theme,
    width: impl Into<Length>,
    height: impl Into<Length>,
) -> w::Container<'a, Option<Space>> {
    w::container(EMPTY_WIDGET)
        .width(width)
        .height(height)
        .style(move |_| ContainerStyle::default().background(theme.border))
}
