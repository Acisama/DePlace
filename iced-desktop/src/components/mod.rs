use deplace_core::RestoreResult;
use iced::{
    Element,
    Length::Fill,
    Subscription, Task,
    widget::{Shader, Stack},
    window,
};
use matrix_sdk::Room;

use crate::{
    AppMessage,
    components::authentification::{DiscoveryAction, DiscoveryMessage},
};
use authentification::{Discovery, Login};
use home::Home;

pub(crate) mod authentification;
mod home;
pub(crate) mod shader;
mod sidebar;

pub enum GenericState<T> {
    Checking,
    Success(T),
    Error(String),
}

#[derive(Default)]
enum Screen {
    #[default]
    Loading,
    Discovery(Discovery),
    Login(Login),
    Home(Home),
}

enum RoomChange {
    ActiveRoom(Option<Room>),
    ActiveServer(Option<Room>),
}

impl Screen {
    fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        match message {
            AppMessage::Discovery(msg) if let Screen::Discovery(discovery) = self => {
                match discovery.update(msg) {
                    DiscoveryAction::None => {}
                    DiscoveryAction::Run(task) => {
                        return task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res)));
                    }
                    DiscoveryAction::SelectedClient(client) => {
                        *self = Screen::Login(Login::new(client));
                    }
                };
            }
            AppMessage::GoToLoading => *self = Screen::Loading,
            AppMessage::GoToDiscovery(client) => *self = Screen::Discovery(Discovery::from(client)),
            AppMessage::GoToLogin(client) => *self = Screen::Login(Login::new(client)),
            AppMessage::GoToHome { state } => *self = Screen::Home(Home::new(*state)),
            AppMessage::Restored(result) => {
                *self = match result {
                    RestoreResult::NeedsLogin(client) => Screen::Login(Login::new(client)),
                    RestoreResult::NoSession | RestoreResult::Success(_) => {
                        let (discovery, task) = Discovery::new();
                        *self = Screen::Discovery(discovery);
                        return task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res)));
                    } // RestoreResult::Success(state) => Screen::Home(Home::new(*state)),
                }
            }
            _ => {}
        };

        Task::none()
    }

    /// Index fed to the loading shader's `u_state`/`u_prev_state` uniforms.
    fn state_index(&self) -> f32 {
        match self {
            Screen::Loading => 0.0,
            Screen::Discovery(_) => 1.0,
            Screen::Login(_) => 2.0,
            Screen::Home(_) => 3.0,
        }
    }

    fn view(&self) -> Element<'_, AppMessage> {
        match self {
            Screen::Loading => "loading".into(),
            Screen::Discovery(discovery) => discovery.view().map(AppMessage::Discovery),
            Screen::Login { .. } => "login".into(),
            Screen::Home { .. } => "home".into(),
        }
    }
}

#[derive(Default)]
pub struct Root {
    screen: Screen,
    loading: shader::LoadingIndicator,
}

impl Root {
    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        self.loading.transition_to(self.screen.state_index());

        self.screen.update(message)
    }

    pub fn subscription(&self) -> Subscription<AppMessage> {
        window::frames().map(|_| AppMessage::Tick)
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let background = Shader::new(self.loading).width(Fill).height(Fill);

        Stack::new()
            .push(background)
            .push(self.screen.view())
            .into()
    }
}
