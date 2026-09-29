use channels::{ChannelsAction, ChannelsMessage, ServerChannels};
use deplace_core::state::ActiveServer;
use macros::iced_cache;
use server_column::{ServerColumn, ServerColumnAction, ServerColumnMessage};

use crate::common::*;

use super::SidebarHelpKey;

mod channels;
mod pill;
mod server_column;

#[derive(Clone, Debug)]
pub enum SidebarMessage {
    ServerColumn(ServerColumnMessage),
    Channels(ChannelsMessage),
    HelpHover(Option<HelpKey>),
}

pub enum SidebarAction {
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<DePlaceRoom>),
    ChangeServer(ActiveServer),
    Run(Task<()>),
    HelpHover(Option<HelpKey>),
}

#[iced_cache(Clone)]
pub struct Sidebar {
    #[hash]
    server_column: ServerColumn,
    #[hash]
    channels: ServerChannels,
}

impl Sidebar {
    pub fn new(state: &AppState) -> Self {
        Self {
            server_column: ServerColumn::new(state),
            channels: ServerChannels::new(state),
        }
    }
}

impl IcedWidget<SidebarMessage, SidebarAction> for Sidebar {
    fn update(&mut self, message: SidebarMessage) -> Option<SidebarAction> {
        match message {
            SidebarMessage::HelpHover(key) => Some(SidebarAction::HelpHover(key)),
            SidebarMessage::ServerColumn(msg) => match self.server_column.update(msg)? {
                ServerColumnAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                ServerColumnAction::SetActiveServer(server) => {
                    Some(SidebarAction::ChangeServer(server))
                }
                ServerColumnAction::SetActiveDm(room) => {
                    Some(SidebarAction::ChangeRoom(Some(room)))
                }
                ServerColumnAction::Run(task) => Some(SidebarAction::Run(task)),
                ServerColumnAction::HelpHover(key) => Some(SidebarAction::HelpHover(key)),
            },
            SidebarMessage::Channels(msg) => match self.channels.update(msg)? {
                ChannelsAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                ChannelsAction::SetActiveRoom(room) => Some(SidebarAction::ChangeRoom(Some(room))),
                ChannelsAction::HelpHover(key) => Some(SidebarAction::HelpHover(key)),
            },
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> Element<'static, SidebarMessage> {
        help(
            help_state,
            theme,
            HelpKey::Sidebar(SidebarHelpKey::Sidebar),
            "The sidebar, used for room navigation",
            w::row![
                w::lazy(
                    (self.server_column.clone(), help_state),
                    move |(server_column, help_state)| {
                        server_column
                            .view(theme, structure, *help_state)
                            .map(SidebarMessage::ServerColumn)
                    }
                ),
                Space::new().width(structure.small_gap),
                w::lazy(
                    (self.channels.clone(), help_state),
                    move |(channels, help_state)| {
                        channels
                            .view(theme, structure, *help_state)
                            .map(SidebarMessage::Channels)
                    }
                ),
            ]
            .height(Fill),
            SidebarMessage::HelpHover,
        )
        .radius(structure.outer_border_radius)
        .into()
    }
}
