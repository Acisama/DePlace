use deplace_core::{room::call::CallMember, state::UserDevice};
use iced::{Alignment, Length, border::Radius};

use crate::{
    common::*,
    components::{PushElement, right_click::right_click, volume_bar::Direction},
};

use crate::modules::home::overlay::{ContextMenu, ContextMenuKind};

use super::ChatMessage;

const MAX_COLUMNS: usize = 3;

pub fn render_call_members(
    room: DePlaceRoom,
    members: Vec<CallMember>,
    own_device: &UserDevice,
    theme: Theme,
    structure: Structure,
    direction: Direction,
) -> Element<'static, ChatMessage> {
    let icon_size = structure.chat.icon_size * 2.0;
    let height = icon_size * 2.5;
    let rounding = icon_size / 6.0;

    let mut column = w::Column::new()
        .width(Fill)
        .padding(icon_size / 2.0)
        .spacing(icon_size / 2.0);

    let gap = structure.icon_gap_ratio * icon_size / 4.0;

    for chunk in members.chunks(MAX_COLUMNS) {
        let mut members_row = w::Row::new().spacing(icon_size / 2.0);
        let fillers = MAX_COLUMNS - chunk.len();

        for member in chunk {
            members_row = members_row.push_element(render_call_member(
                member.clone(),
                room.clone(),
                theme,
                structure,
                icon_size,
                rounding,
                height,
                gap,
                own_device,
            ));
        }

        let row = if fillers > 0 {
            w::row![
                Space::new()
                    .width(Length::FillPortion(fillers as u16))
                    .height(height),
                w::container(members_row).width(Length::FillPortion((chunk.len() * 2) as u16)),
                Space::new()
                    .width(Length::FillPortion(fillers as u16))
                    .height(height),
            ]
            .boxed()
        } else {
            members_row.width(Fill).boxed()
        };

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
        w::container(themed_scrollable(column, theme, structure))
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

#[allow(clippy::too_many_arguments)]
fn render_call_member(
    member: CallMember,
    room: DePlaceRoom,
    theme: Theme,
    structure: Structure,
    icon_size: f32,
    rounding: f32,
    height: impl Into<Length>,
    gap: f32,
    own_device: &UserDevice,
) -> w::Element<'static, ChatMessage> {
    let color = member.color().to_iced();
    let is_speaking = member.is_speaking();

    let is_own = member.is_own(own_device);

    right_click(
        w::container(w::stack![
            w::container(member.render_icon(icon_size))
                .colored_rounded(color.scale_lightness(0.15), rounding)
                .width(Length::FillPortion(2))
                .height(height)
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
            w::container(
                w::container(
                    w::text(member.displayname())
                        .size(structure.font_size)
                        .color(theme.text.normal)
                )
                .style(move |_| ContainerStyle {
                    background: Some(theme.solid_bg.into()),
                    border: Border {
                        color: theme.border.into(),
                        width: structure.border_thickness,
                        radius: (rounding - structure.small_gap).into()
                    },
                    ..Default::default()
                })
                .padding(
                    padding::horizontal(structure.small_gap).vertical(structure.small_gap / 2.0)
                )
            )
            .padding(structure.small_gap)
            .height(Fill)
            .align_y(Alignment::End)
        ])
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
        move |position| {
            ChatMessage::ContextMenu(ContextMenu {
                position,
                kind: ContextMenuKind::call_participant(room.clone(), member.clone(), is_own),
            })
        },
    )
    .boxed()
}
