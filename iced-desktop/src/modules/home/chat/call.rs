use deplace_core::{
    call::VolumeCell, room::call::CallMember, settings::NameDecoration, state::UserDevice,
};
use iced::{Alignment, Length, Radians, border::Radius, gradient::Linear};

use crate::{
    common::*,
    components::{
        PushElement, call_grid::call_grid, phosphor_icon, right_click::right_click, volume_bar,
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
    decoration: NameDecoration,
) -> Element<'static, ChatMessage> {
    let own_in_call = room.call().own_user_device_is_in_call();
    let icon_size = structure.chat.icon_size * 2.0;
    let rounding = icon_size / 8.0;
    let gap = structure.icon_gap_ratio * icon_size / 4.0;

    let grid = || {
        let own_device = own_device.clone();

        let room_clone = room.clone();

        call_grid(
            members,
            |member| (*member.device_id).clone(),
            move |member| {
                render_call_member(
                    member,
                    room_clone.clone(),
                    theme,
                    structure,
                    icon_size,
                    rounding,
                    gap,
                    &own_device,
                    own_in_call,
                )
            },
        )
        .spacing(rounding)
        .padding(rounding)
        .height(Fill)
    };

    let join_stuff = || {
        w::column![
            room.render_name_decorated(
                structure.large_font_size * 1.5,
                decoration,
                theme.text.normal
            ),
            w::text(if !room.has_call() {
                "No one is currently in this call"
            } else {
                "Ready to join the conversation?"
            })
            .size(structure.font_size)
            .color(theme.text.normal),
            animated_button("Join Call")
                .on_press(ChatMessage::JoinCall)
                .style(move |_, status| ButtonStyle {
                    background: if status.active() {
                        Some(theme.colors.success.into())
                    } else {
                        Some(theme.success_blended().into())
                    },
                    border: Border {
                        color: theme.colors.success.into(),
                        width: structure.border_thickness,
                        radius: structure.inner_border_radius.into()
                    },
                    text_color: if status.active() {
                        theme.solid_bg.into()
                    } else {
                        theme.colors.success.into()
                    },
                    ..Default::default()
                })
        ]
        .width(Fill)
        .spacing(structure.small_gap)
        .align_x(Alignment::Center)
    };

    let style = move |bg| ContainerStyle {
        background: Some(bg),
        border: border::rounded(match direction {
            Direction::Right => Radius::default().left(structure.outer_border_radius),
            Direction::Left => Radius::default().right(structure.outer_border_radius),
            Direction::Top => Radius::default().bottom(structure.outer_border_radius),
            Direction::Bottom => Radius::default().top(structure.outer_border_radius),
        }),
        ..Default::default()
    };

    let content = w::stack![
        w::column![
            grid(),
            own_in_call.then(|| {
                w::row![
                    animated_button("Leave Call")
                        .on_press(ChatMessage::LeaveCall)
                        .style(move |_, status| ButtonStyle {
                            background: if status.active() {
                                Some(theme.colors.error.into())
                            } else {
                                Some(theme.error_blended().into())
                            },
                            border: Border {
                                color: theme.colors.error.into(),
                                width: structure.border_thickness,
                                radius: structure.inner_border_radius.into(),
                            },
                            text_color: if status.active() {
                                theme.solid_bg.into()
                            } else {
                                theme.colors.error.into()
                            },
                            ..Default::default()
                        })
                ]
                .align_y(Alignment::Center)
                .padding(padding::bottom(rounding))
            })
        ]
        .align_x(Alignment::Center),
        (!own_in_call).then(|| {
            w::container(join_stuff())
                .style(move |_| style(theme.background.into()))
                .width(Fill)
                .height(Fill)
                .align_y(Alignment::Center)
        })
    ]
    .boxed();

    let call_content = w::container(
        w::container(content)
            .width(Fill)
            .height(Fill)
            .style(move |_| style(theme.solid_bg.into())),
    )
    .padding(structure.border_thickness)
    .width(Fill)
    .height(Fill);

    match direction {
        Direction::Right => w::row![
            call_content,
            divider(theme, structure.border_thickness, Fill)
        ]
        .boxed(),
        Direction::Left => w::row![
            divider(theme, structure.border_thickness, Fill),
            call_content
        ]
        .boxed(),
        Direction::Top => w::column![
            divider(theme, Fill, structure.border_thickness),
            call_content
        ]
        .boxed(),
        Direction::Bottom => w::column![
            call_content,
            divider(theme, Fill, structure.border_thickness)
        ]
        .boxed(),
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
    own_in_call: bool,
) -> w::Element<'static, ChatMessage> {
    let color = member.color().to_iced();
    let is_speaking = member.is_speaking();
    let playback_muted = member.playback_muted();
    let is_muted = member.is_muted();

    let is_own = member.is_own(own_device);

    let mut name_row = w::Row::new()
        .spacing(structure.small_gap / 2.0)
        .align_y(Alignment::Center);

    if own_in_call && is_muted {
        name_row = name_row.push_element(
            phosphor_icon(MUTE_ICON, structure.font_size * 0.9).color(theme.colors.error),
        );
    }

    name_row = name_row.push_element(
        w::text(member.displayname())
            .size(structure.font_size)
            .color(theme.text.normal),
    );

    if own_in_call && playback_muted {
        name_row = name_row.push_element(
            w::text("(muted by you)")
                .size(structure.font_size)
                .color(theme.text.dim),
        );
    }

    let has_border = playback_muted || is_speaking;

    let border_color = if own_in_call {
        if playback_muted {
            theme.colors.error.into()
        } else if is_speaking {
            theme.colors.success.into()
        } else {
            Color::TRANSPARENT
        }
    } else {
        Color::TRANSPARENT
    };

    let background = Linear::new(Radians::PI / 4.0)
        .add_stop(0.0, color.scale_lightness(0.1))
        .add_stop(0.7, color)
        .add_stop(1.0, color.scale_lightness(0.6));

    right_click(
        w::stack![
            w::container(
                w::container(
                    w::container(member.render_icon(icon_size))
                        .width(icon_size + 2.0 * gap)
                        .height(icon_size + 2.0 * gap)
                        .colored_rounded(theme.solid_bg, icon_size + 2.0 * gap)
                        .align_x(Alignment::Center)
                        .align_y(Alignment::Center)
                )
                .colored_rounded(
                    background,
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
            .align_y(Alignment::End),
            own_in_call.then(|| w::container(volume_bar(
                if member.is_muted() {
                    Arc::new(VolumeCell::default())
                } else {
                    member.volume()
                },
                icon_size / 4.0,
                icon_size * 2.0,
                volume_bar::Direction::Top,
                Color::from_rgb8(0, 255, 0),
                // theme.colors.success,
                // theme.colors.error,
                Color::from_rgb8(255, 0, 0),
                if member.is_muted() {
                    theme.colors.error
                } else {
                    theme.solid_bg
                },
                structure.semi_small_border_radius(),
                theme.border,
                structure.border_thickness,
            ))
            .padding(gap * 4.0)
            .height(Fill)
            .width(Fill)
            .align_y(Alignment::End)
            .align_x(Alignment::End))
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
