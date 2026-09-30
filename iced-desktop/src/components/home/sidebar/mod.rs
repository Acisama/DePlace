use account::{AccountAction, AccountMessage, AccountView};
use channels::{ChannelsAction, ChannelsMessage, ServerChannels};
use deplace_core::state::ActiveServer;
use macros::iced_cache;
use server_column::{ServerColumn, ServerColumnAction, ServerColumnMessage};

use crate::common::*;

use super::SidebarHelpKey;

mod account;
mod channels;
mod pill;
mod server_column;

#[derive(Clone, Debug)]
pub enum SidebarMessage {
    ServerColumn(ServerColumnMessage),
    Channels(ChannelsMessage),
    HelpHover(Option<HelpKey>),
    Account(AccountMessage),
}

pub enum SidebarAction {
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<DePlaceRoom>),
    ChangeServer(ActiveServer),
    Run(Task<()>),
    HelpHover(Option<HelpKey>),
    OpenSettings,
}

#[iced_cache(Clone)]
pub struct Sidebar {
    #[hash]
    server_column: ServerColumn,
    #[hash]
    channels: ServerChannels,
    #[hash]
    account: AccountView,
}

impl Sidebar {
    pub fn new(state: &AppState) -> Self {
        Self {
            server_column: ServerColumn::new(state),
            channels: ServerChannels::new(state),
            account: AccountView::new(state),
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
            SidebarMessage::Account(msg) => match self.account.update(msg)? {
                AccountAction::NeedsMedia(media) => Some(SidebarAction::NeedsMedia(media)),
                AccountAction::OpenSettings => Some(SidebarAction::OpenSettings),
                AccountAction::HelpHover(help) => Some(SidebarAction::HelpHover(help)),
            },
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> Element<'static, SidebarMessage> {
        let content = w::row![
            w::lazy(
                (self.server_column.clone(), help_state),
                move |(server_column, help_state)| {
                    server_column
                        .view(theme, structure, *help_state)
                        .map(SidebarMessage::ServerColumn)
                }
            ),
            w::column![
                w::lazy(
                    (self.channels.clone(), help_state),
                    move |(channels, help_state)| {
                        channels
                            .view(theme, structure, *help_state)
                            .map(SidebarMessage::Channels)
                    }
                ),
                w::lazy(
                    (self.account.clone(), help_state),
                    move |(account, help_state)| {
                        account
                            .view(theme, structure, *help_state)
                            .map(SidebarMessage::Account)
                    }
                ),
            ]
            .height(Fill)
            .spacing(structure.small_gap),
        ]
        .spacing(structure.small_gap)
        .height(Fill);

        help(
            help_state,
            theme,
            HelpKey::Sidebar(SidebarHelpKey::Sidebar),
            "The sidebar, used for room navigation",
            content,
            SidebarMessage::HelpHover,
        )
        .radius(structure.outer_border_radius)
        .into()
    }
}
