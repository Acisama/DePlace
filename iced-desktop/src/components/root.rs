use deplace_core::RestoreResult;
use iced::widget::Shader;
use iced::window;

use crate::common::*;

use crate::components::authentification::discovery::{DiscoveryAction, DiscoveryMessage};
use crate::components::authentification::login::{LoginAction, LoginMessage};
use crate::components::authentification::verification::{VerificationAction, VerificationMessage};
use crate::components::home::HomeAction;
use crate::components::shader;
use crate::{
    AppMessage,
    components::{
        authentification::{discovery::Discovery, login::Login, verification::Verification},
        home::Home,
    },
};

#[derive(Default, Hash)]
enum Screen {
    #[default]
    Loading,
    Discovery(Discovery),
    Login(Login),
    Verification(Verification),
    Home(Box<Home>),
}

impl Screen {
    fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        match message {
            AppMessage::Home(msg) if let Screen::Home(home) = self => {
                match home.update(msg) {
                    HomeAction::None => {}
                    HomeAction::EmptyRun(task) => {
                        return task.map(|_| AppMessage::None);
                    }
                };
            }
            AppMessage::Discovery(msg) if let Screen::Discovery(discovery) = self => {
                match discovery.update(msg) {
                    DiscoveryAction::None => {}
                    DiscoveryAction::Run(task) => {
                        return task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res)));
                    }
                    DiscoveryAction::ClientSelected(client) => {
                        let (login, task) = Login::new(client);
                        *self = Screen::Login(login);
                        return task.map(AppMessage::Login);
                    }
                };
            }
            AppMessage::Login(msg) if let Screen::Login(login) = self => match login.update(msg) {
                LoginAction::None => {}
                LoginAction::Run(task) => {
                    return task.map(|res| AppMessage::Login(LoginMessage::from(res)));
                }
                LoginAction::BackToDiscovery(client) => {
                    let (dis, task) = Discovery::from_client(client);
                    *self = Screen::Discovery(dis);
                    return task.map(AppMessage::Discovery);
                }
                LoginAction::LoginSuccess(state) => {
                    *self = Screen::Verification(Verification::new(state));
                }
            },
            AppMessage::Verification(msg) if let Screen::Verification(verification) = self => {
                match verification.update(msg) {
                    VerificationAction::None => {}
                    VerificationAction::Run(task) => {
                        return task
                            .map(|res| AppMessage::Verification(VerificationMessage::from(res)));
                    }
                    VerificationAction::Success(state) => {
                        let (home, task) = Home::new(state);
                        *self = Screen::Home(Box::new(home));
                        return task.map(AppMessage::Home);
                    }
                }
            }
            AppMessage::GoToLoading => *self = Screen::Loading,
            AppMessage::GoToLogin(client) => {
                let (login, task) = Login::new(client);
                *self = Screen::Login(login);
                return task.map(AppMessage::Login);
            }
            AppMessage::Restored(result) => match result {
                RestoreResult::NeedsLogin(client) => {
                    let (login, task) = Login::new(client);
                    *self = Screen::Login(login);
                    return task.map(AppMessage::Login);
                }
                RestoreResult::NoSession => {
                    let (discovery, task) = Discovery::new();
                    *self = Screen::Discovery(discovery);
                    return task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res)));
                }
                RestoreResult::Success(state) => {
                    let (home, task) = Home::new(*state);
                    *self = Screen::Home(Box::new(home));
                    return task.map(AppMessage::Home);
                }
            },
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
            Screen::Verification(_) => 3.0,
            Screen::Home(_) => 4.0,
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, AppMessage> {
        match self {
            Screen::Loading => Space::new().into(),
            Screen::Discovery(discovery) => {
                discovery.view(theme, structure).map(AppMessage::Discovery)
            }
            Screen::Login(login) => login.view(theme, structure).map(AppMessage::Login),
            Screen::Verification(verification) => verification
                .view(theme, structure)
                .map(AppMessage::Verification),
            Screen::Home(home) => home.view(theme, structure).map(AppMessage::Home),
        }
    }
}

pub struct Root {
    screen: Screen,
    loading: shader::LoadingIndicator,
    theme: Theme,
    structure: Structure,
}

impl Default for Root {
    fn default() -> Self {
        Self {
            screen: Screen::default(),
            loading: shader::LoadingIndicator::default(),
            theme: Theme::new(),
            structure: Structure::new(),
        }
    }
}

impl Root {
    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        self.loading.transition_to(self.screen.state_index());

        if let AppMessage::TabPressed { shift } = message {
            return if shift {
                w::operation::focus_previous()
            } else {
                w::operation::focus_next()
            };
        }

        self.screen.update(message)
    }

    pub fn title(&self, _: window::Id) -> String {
        if let Screen::Home(home) = &self.screen {
            return home.title();
        }
        "DePlace".to_string()
    }

    pub fn subscription(&self) -> Subscription<AppMessage> {
        Subscription::batch([
            window::frames().map(|_| AppMessage::Tick),
            iced::keyboard::listen().filter_map(|event| match event {
                iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                    modifiers,
                    ..
                } => Some(AppMessage::TabPressed {
                    shift: modifiers.shift(),
                }),
                _ => None,
            }),
        ])
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let background = Shader::new(self.loading).width(Fill).height(Fill);
        let theme = self.theme;
        let structure = self.structure;

        Stack::new()
            .push(background)
            .push(w::lazy(&self.screen, move |screen| {
                screen.view(theme, structure)
            }))
            .into()
    }
}
