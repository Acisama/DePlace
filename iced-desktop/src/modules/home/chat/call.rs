use deplace_core::{room::call::CallMember, state::UserDevice};
use iced::{Alignment, Length, border::Radius};

use crate::{
    common::*,
    components::{
        PushElement, call_grid::call_grid, phosphor_icon, right_click::right_click,
        volume_bar::Direction,
    },
    modules::home::MUTE_ICON,
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
    let rounding = icon_size / 8.0;
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
    .spacing(rounding)
    .padding(rounding)
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
    let is_muted = member.is_muted();

    let is_own = member.is_own(own_device);

    let mut name_row = w::Row::new()
        .spacing(structure.small_gap / 2.0)
        .align_y(Alignment::Center);

    if is_muted {
        name_row = name_row.push_element(
            phosphor_icon(MUTE_ICON, structure.font_size * 0.9).color(theme.colors.error),
        );
    }

    name_row = name_row.push_element(
        w::text(member.displayname())
            .size(structure.font_size)
            .color(theme.text.normal),
    );

    let has_border = playback_muted || is_speaking;

    let border_color = if playback_muted {
        theme.colors.error.into()
    } else if is_speaking {
        theme.colors.success.into()
    } else {
        Color::TRANSPARENT
    };

    right_click(
        w::stack![
            w::container(
                w::container(member.render_icon(icon_size))
                    .colored_rounded(
                        color.scale_lightness(0.2),
                        if has_border {
                            rounding - gap * 2.0
                        } else {
                            rounding
                        }
                    )
                    .width(Fill)
                    .height(Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::Center)
            )
            .padding(if has_border { gap * 2.5 } else { 0.0 })
            .style(move |_| ContainerStyle::default().border(Border {
                color: border_color,
                width: if has_border { gap } else { 0.0 },
                radius: rounding.into(),
            })),
            w::container(
                w::container(name_row)
                    .style(move |_| ContainerStyle {
                        background: Some(theme.solid_bg.into()),
                        border: Border {
                            color: theme.border.into(),
                            width: structure.border_thickness,
                            radius: structure.semi_small_border_radius().into()
                        },
                        ..Default::default()
                    })
                    .padding(
                        padding::horizontal(structure.small_gap)
                            .vertical(structure.small_gap / 2.0)
                    )
            )
            .padding(gap * 4.0)
            .height(Fill)
            .align_y(Alignment::End)
        ],
        move |position| {
            ChatMessage::ContextMenu(ContextMenu {
                position,
                kind: ContextMenuKind::call_participant(room.clone(), member.clone(), is_own),
            })
        },
    )
    .boxed()
}
