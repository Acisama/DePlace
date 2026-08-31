use channels::{ChannelsAction, ChannelsMessage, ServerChannels};
use deplace_core::state::ActiveServer;
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
    NeedAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for SidebarMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        SidebarMessage::NeedAvatar(uri)
    }
}

pub enum SidebarAction {
    None,
    Run(Task<()>),
}

#[derive(Clone)]
pub struct Sidebar {
    state: AppState,

    server_column: ServerColumn,
    channels: ServerChannels,
}

impl Hash for Sidebar {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.server_column.hash(state);
        self.channels.hash(state);
    }
}

impl Sidebar {
    pub fn new(state: &AppState) -> Self {
        Self {
            server_column: ServerColumn::new(state),
            channels: ServerChannels::new(state),

            state: state.clone(),
        }
    }

    pub fn set_active_server_task(&mut self, server: ActiveServer) -> SidebarAction {
        let state = self.state.clone();
        SidebarAction::Run(Task::future(async move {
            state.set_active_server(server, true).await;
        }))
    }

    pub fn set_active_room_task(&mut self, room: Option<Room>) -> SidebarAction {
        let state = self.state.clone();
        SidebarAction::Run(Task::future(async move {
            state.set_active_room(room).await;
        }))
    }

    pub fn fetch_avatar_task(&mut self, uri: OwnedMxcUri) -> SidebarAction {
        let avatar_cache = self.state.avatar_cache().clone();
        SidebarAction::Run(Task::future(
            async move { avatar_cache.load_avatar(&uri).await },
        ))
    }

    pub fn update(&mut self, message: SidebarMessage) -> SidebarAction {
        match message {
            SidebarMessage::ChangeActiveRoom(room) => self.set_active_room_task(room),
            SidebarMessage::ChangeActiveServer(server) => self.set_active_server_task(server),
            SidebarMessage::NeedAvatar(uri) => self.fetch_avatar_task(uri),
            SidebarMessage::ServerColumn(msg) => match self.server_column.update(msg) {
                ServerColumnAction::NeedAvatar(uri) => self.fetch_avatar_task(uri),
                ServerColumnAction::SetActiveServer(server) => self.set_active_server_task(server),
                ServerColumnAction::None => SidebarAction::None,
            },
            SidebarMessage::Channels(msg) => match self.channels.update(msg) {
                ChannelsAction::FetchAvatar(uri) => self.fetch_avatar_task(uri),
                ChannelsAction::SetActiveRoom(room) => self.set_active_room_task(Some(room)),
            },
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, SidebarMessage> {
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
