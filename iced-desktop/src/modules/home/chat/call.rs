use deplace_core::{room::call::CallMember, state::UserDevice};
use iced::{Alignment, Length, border::Radius};

use crate::{
    common::*,
    components::{call_grid::call_grid, right_click::right_click, volume_bar::Direction},
};

use crate::modules::home::overlay::{ContextMenu, ContextMenuKind};

use super::ChatMessage;

pub fn render_call_members(
    room: DePlaceRoom,
    members: Vec<CallMember>,
    own_device: &UserDevice,
    theme: Theme,
    structure: Structure,
    direction: Direction,
) -> Element<'static, ChatMessage> {
    let icon_size = structure.chat.icon_size * 2.0;
    let rounding = icon_size / 6.0;
    let gap = structure.icon_gap_ratio * icon_size / 4.0;
    let own_device = own_device.clone();

    let grid = call_grid(
        members,
        |member| (*member.device_id).clone(),
        move |member| {
            render_call_member(
                member,
                room.clone(),
                theme,
                structure,
                icon_size,
                rounding,
                gap,
                &own_device,
            )
        },
    )
    .spacing(icon_size / 2.0)
    .padding(icon_size / 2.0)
    .height(Fill);

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
        w::container(grid)
            .width(Fill)
            .height(Fill)
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
    gap: f32,
    own_device: &UserDevice,
) -> w::Element<'static, ChatMessage> {
    let color = member.color().to_iced();
    let is_speaking = member.is_speaking();
    let playback_muted = member.playback_muted();

    let is_own = member.is_own(own_device);

    right_click(
        w::container(w::stack![
            w::container(member.render_icon(icon_size))
                .colored_rounded(color.scale_lightness(0.2), rounding)
                .width(Fill)
                .height(Fill)
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
                color: if playback_muted {
                    theme.colors.error.into()
                } else if is_speaking {
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
