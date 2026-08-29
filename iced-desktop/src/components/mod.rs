use deplace_core::RestoreResult;
use iced::{
    Element,
    Length::Fill,
    Subscription, Task,
    widget::{Shader, Stack},
    window,
};
use matrix_sdk::Room;

use crate::AppMessage;
use authentification::{Discovery, Login};
use home::Home;

mod authentification;
mod home;
pub(crate) mod shader;
mod sidebar;

#[derive(Default)]
pub enum GenericState<T> {
    #[default]
    Unchecked,
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
    fn update(&mut self, message: AppMessage) {
        match message {
            AppMessage::GoToLoading => *self = Screen::Loading,
            AppMessage::GoToDiscovery(client) => *self = Screen::Discovery(Discovery::new(client)),
            AppMessage::GoToLogin(client) => *self = Screen::Login(Login::new(client)),
            AppMessage::GoToHome { state } => *self = Screen::Home(Home::new(*state)),
            AppMessage::Restored(result) => {
                *self = match result {
                    RestoreResult::NeedsLogin(client) => Screen::Login(Login::new(client)),
                    RestoreResult::NoSession => Screen::Loading,
                    RestoreResult::Success(state) => Screen::Home(Home::new(*state)),
                }
            }
            _ => {}
        };
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
}

#[derive(Default)]
pub struct Root {
    screen: Screen,
    loading: shader::LoadingIndicator,
}

impl Root {
    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        self.screen.update(message);
        self.loading.transition_to(self.screen.state_index());

        Task::none()
    }

    pub fn subscription(&self) -> Subscription<AppMessage> {
        window::frames().map(|_| AppMessage::Tick)
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let background = Shader::new(self.loading).width(Fill).height(Fill);
        let content: Element<'_, AppMessage> = match &self.screen {
            Screen::Loading => "loading".into(),
            Screen::Discovery { .. } => "discovery".into(),
            Screen::Login { .. } => "login".into(),
            Screen::Home { .. } => "home".into(),
        };

        Stack::new().push(background).push(content).into()
    }
}
