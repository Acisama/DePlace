use deplace_core::RestoreResult;
use iced::{Element, Task, window};
use matrix_sdk::reqwest::Url;

use crate::AppMessage;
use authentification::{Discovery, Login};
use home::Home;

mod authentification;
mod home;

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

impl Screen {
    fn update(&mut self, message: AppMessage) {
        match message {
            AppMessage::Start(_) => {}
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
        };
    }
}

#[derive(Default)]
pub struct Root {
    screen: Screen,
}

impl Root {
    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        self.screen.update(message);

        Task::none()
    }

    pub fn view(&self, _window: window::Id) -> Element<AppMessage> {
        match &self.screen {
            Screen::Loading => "loading".into(),
            Screen::Discovery { .. } => "discovery".into(),
            Screen::Login { .. } => "login".into(),
            Screen::Home { .. } => "home".into(),
        }
    }
}
