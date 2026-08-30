use iced::widget::svg;

use crate::common::*;

pub enum SidebarMessage {
    ActiveRoomChange(Option<Room>),
    ActiveServerChange(Option<Room>),
}

pub struct Sidebar {
    state: AppState,
}

impl Hash for Sidebar {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.state.room_version().hash(state);
    }
}

impl Sidebar {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, SidebarMessage> {
        w::row![
            render_server_column(theme, structure),
            Space::new().width(structure.small_gap),
            floating_tile(theme, structure, "test")
        ]
        .into()
    }
}

fn render_server_column(theme: Theme, structure: Structure) -> Element<'static, SidebarMessage> {
    let icon_handle = iced::advanced::svg::Handle::from_memory(include_bytes!(
        "../../../../assets/deplace_icon.svg"
    ));

    floating_tile(
        theme,
        structure,
        w::column![
            svg(icon_handle)
                .width(structure.server_column.icon_size)
                .height(structure.server_column.icon_size),
            "test"
        ],
    )
    .width(structure.server_column_width())
    .height(Fill)
    .padding(structure.small_gap * 1.5)
    .into()
}
