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
    ChangeActiveRoom(Option<Room>),
    ChangeActiveServer(ActiveServer),
}

pub enum SidebarAction {
    Run(Task<()>),
    NeedsMedia(NeedsMedia),
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

    pub fn load_media(&mut self, media: &MediaLoaded) {
        self.server_column.load_media(media);
        self.channels.load_media(media);
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
            SidebarMessage::ChangeActiveRoom(room) => self.set_active_room_task(room),
            SidebarMessage::ChangeActiveServer(server) => self.set_active_server_task(server),
            SidebarMessage::ServerColumn(msg) => {
                if let Some(action) = self.server_column.update(msg) {
                    match action {
                        ServerColumnAction::Run(task) => Some(SidebarAction::Run(task)),
                        ServerColumnAction::SetActiveServer(server) => {
                            self.set_active_server_task(server)
                        }
                        ServerColumnAction::NeedsMedia(media) => {
                            Some(SidebarAction::NeedsMedia(media))
                        }
                    }
                } else {
                    None
                }
            }
            SidebarMessage::Channels(msg) => {
                if let Some(action) = self.channels.update(msg) {
                    match action {
                        ChannelsAction::SetActiveRoom(room) => {
                            self.set_active_room_task(Some(room))
                        }
                        ChannelsAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                    }
                } else {
                    None
                }
            }
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
