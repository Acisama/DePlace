use deplace_core::state::{ActiveServer, ActiveServerId};
use iced::widget::svg;

use super::pill::PillCanvas;
use crate::{common::*, components::home::sidebar::SidebarMessage};

fn pill(
    theme: Theme,
    structure: Structure,
    active: bool,
    hovered: bool,
    has_messages: bool,
    content: MouseArea<'static, SidebarMessage>,
) -> Stack<'static, SidebarMessage> {
    let target = if active {
        structure.server_column.icon_size
    } else if hovered {
        structure.server_column.icon_size / 2.0
    } else if has_messages {
        structure.small_gap / 2.0
    } else {
        0.0
    };

    Stack::new()
        .push(w::row![Space::new().width(structure.small_gap), content])
        .push(w::column![
            Canvas::new(PillCanvas {
                target,
                width: structure.small_gap / 2.0,
                color: theme.pill_color,
                radius: structure.small_gap / 4.0,
            })
            .width(structure.small_gap / 2.0)
            .height(structure.server_column.icon_size)
        ])
}

pub fn render_server_column(
    theme: Theme,
    structure: Structure,
    sorted_rooms: Vec<Room>,
    active_server: ActiveServer,
    hovered_server: &Option<ActiveServerId>,
    avatar_cache: &AvatarCache,
) -> Element<'static, SidebarMessage> {
    let icon_handle = iced::advanced::svg::Handle::from_memory(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../assets/deplace_icon.svg"
    )));
    let icon_size = structure.server_column.icon_size;

    let mut column = w::column![
        pill(
            theme,
            structure,
            active_server.is_dms(),
            hovered_server
                .as_ref()
                .map(|id| id.is_dms())
                .unwrap_or(false),
            false,
            w::mouse_area(svg(icon_handle).width(icon_size).height(icon_size))
                .interaction(Interaction::Pointer)
                .on_press(SidebarMessage::ChangeActiveServer(ActiveServer::Dms))
                .on_enter(SidebarMessage::ServerHovered(ActiveServerId::Dms))
                .on_exit(SidebarMessage::ServerHoverEnded(ActiveServerId::Dms))
        ),
        w::row![
            Space::new().width(structure.small_gap),
            w::container(
                Space::new()
                    .width(icon_size)
                    .height(structure.divider_width)
            )
            .style(move |_| w::container::Style {
                background: Some(theme.border.into()),
                border: Border {
                    radius: (structure.small_gap / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            })
        ]
    ]
    .spacing(structure.gap);

    for room in sorted_rooms {
        let id = room.room_id().to_owned();

        column = column.push(pill(
            theme,
            structure,
            active_server.is_server(&id),
            hovered_server
                .as_ref()
                .map(|sid| sid.is_server(&id))
                .unwrap_or(false),
            false,
            w::mouse_area(room.render_icon(icon_size, avatar_cache))
                .interaction(Interaction::Pointer)
                .on_press(SidebarMessage::ChangeActiveServer(ActiveServer::Server(
                    room,
                )))
                .on_enter(SidebarMessage::ServerHovered(ActiveServerId::Server(
                    id.clone(),
                )))
                .on_exit(SidebarMessage::ServerHoverEnded(ActiveServerId::Server(id))),
        ));
    }

    floating_tile(theme, structure, column)
        .width(structure.server_column_width())
        .height(Fill)
        .padding(Padding {
            top: structure.small_gap * 1.5,
            bottom: structure.small_gap / 2.0,
            left: structure.small_gap / 2.0,
            right: structure.small_gap / 2.0,
        })
        .into()
}
