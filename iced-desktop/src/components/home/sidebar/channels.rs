use deplace_core::state::ActiveServer;

use crate::common::*;

use super::SidebarMessage;

pub fn render_channels(
    theme: Theme,
    structure: Structure,
    active_server: ActiveServer,
    channels: Vec<Room>,
) -> Element<'static, SidebarMessage> {
    floating_tile(theme, structure, w::row![text(active_server.get_name())]).into()
}
