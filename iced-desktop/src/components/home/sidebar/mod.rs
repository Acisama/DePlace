use channels::{ChannelsAction, ChannelsMessage, ServerChannels};
use deplace_core::state::ActiveServer;
use macros::iced_cache;
use server_column::{ServerColumn, ServerColumnAction, ServerColumnMessage};

use crate::common::*;

mod channels;
mod pill;
mod server_column;

#[derive(Clone, Debug)]
pub enum SidebarMessage {
    ServerColumn(ServerColumnMessage),
    Channels(ChannelsMessage),
}

pub enum SidebarAction {
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<DePlaceRoom>),
    ChangeServer(ActiveServer),
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
            SidebarMessage::ServerColumn(msg) => match self.server_column.update(msg)? {
                ServerColumnAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                ServerColumnAction::SetActiveServer(server) => {
                    Some(SidebarAction::ChangeServer(server))
                }
                ServerColumnAction::SetActiveDm(room) => {
                    Some(SidebarAction::ChangeRoom(Some(room)))
                }
            },
            SidebarMessage::Channels(msg) => match self.channels.update(msg)? {
                ChannelsAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                ChannelsAction::SetActiveRoom(room) => Some(SidebarAction::ChangeRoom(Some(room))),
            },
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, SidebarMessage> {
        w::row![
            w::lazy(self.server_column.clone(), move |server_column| {
                server_column
                    .view(theme, structure)
                    .map(SidebarMessage::ServerColumn)
            }),
            Space::new().width(structure.small_gap),
            w::lazy(self.channels.clone(), move |channels| {
                channels
                    .view(theme, structure)
                    .map(SidebarMessage::Channels)
            }),
        ]
        .height(Fill)
        .into()
    }
}
