use deplace_core::room::call::CallMember;
use iced::{Alignment, Length, border::Radius};

use crate::{
    common::*,
    components::{InsetShadow, PushElement, volume_bar::Direction},
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
    let height = icon_size * 2.0;
    let rounding = icon_size / 4.0;

    let mut column = w::Column::new()
        .width(Fill)
        .padding(icon_size / 2.0)
        .spacing(icon_size / 2.0);

    let total_length = members.len();

    for chunk in members.chunks(MAX_COLUMNS) {
        let mut row = w::Row::new().spacing(icon_size / 2.0);

        // A trailing row with fewer than `MAX_COLUMNS` members is padded
        // out with a leading and trailing filler so every member cell is
        // still the same width as a cell in a full row, while the actual
        // members end up centered. Each member cell counts for 2 portions
        // (instead of 1) so an odd number of missing cells still has a
        // whole number of portions to give each filler, splitting evenly
        // instead of rounding to one side.
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

            row = row.push_element(
                w::stack![
                    w::container(member.render_icon(icon_size))
                        .colored_rounded(color.to_iced().scale_lightness(0.2), rounding)
                        .width(Fill)
                        .height(height)
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center),
                    Canvas::new(InsetShadow::new(rounding, color, icon_size / 8.0, 8))
                        .width(Fill)
                        .height(height),
                ]
                .width(Length::FillPortion(2)),
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
