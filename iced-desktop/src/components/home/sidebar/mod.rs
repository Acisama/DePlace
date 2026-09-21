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
    Run(Task<()>),
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<Room>),
    ChangeServer(ActiveServer),
}

#[iced_cache(Clone)]
pub struct Sidebar {
    state: AppState,

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

            state: state.clone(),
        }
    }

    pub fn set_active_server_task(&mut self, server: ActiveServer) -> Option<SidebarAction> {
        let state = self.state.clone();
        Some(SidebarAction::Run(Task::future(async move {
            state.set_active_server(server, true).await;
        })))
    }

    pub fn set_active_room_task(&mut self, room: Option<Room>) -> Option<SidebarAction> {
        let state = self.state.clone();
        Some(SidebarAction::Run(Task::future(async move {
            state.set_active_room(room).await;
        })))
    }
}

impl IcedWidget<SidebarMessage, SidebarAction> for Sidebar {
    fn update(&mut self, message: SidebarMessage) -> Option<SidebarAction> {
        match message {
            // SidebarMessage::ServerColumn(msg) => {
            //     if let Some(action) = self.server_column.update(msg) {
            //         match action {
            //             ServerColumnAction::Run(task) => Some(SidebarAction::Run(task)),
            //             ServerColumnAction::SetActiveServer(server) => {
            //                 self.set_active_server_task(server)
            //             }
            //             ServerColumnAction::NeedsMedia(media) => {
            //                 Some(SidebarAction::NeedsMedia(media))
            //             }
            //         }
            //     } else {
            //         None
            //     }
            // }
            // SidebarMessage::Channels(msg) => {
            //     if let Some(action) = self.channels.update(msg) {
            //         match action {
            //             ChannelsAction::SetActiveRoom(room) => {
            //                 self.set_active_room_task(Some(room))
            //             }
            //             ChannelsAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
            //         }
            //     } else {
            //         None
            //     }
            // }
            SidebarMessage::ServerColumn(msg) => match self.server_column.update(msg)? {
                ServerColumnAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                ServerColumnAction::SetActiveServer(server) => {
                    Some(SidebarAction::ChangeServer(server))
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
