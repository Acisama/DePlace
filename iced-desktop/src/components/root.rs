use deplace_core::RestoreResult;
use iced::advanced::subscription;
use iced::widget::Shader;
use iced::window;
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, prelude::*};

use crate::{SOCKET_NAME, SocketListener, common::*};

use crate::components::authentification::discovery::{DiscoveryAction, DiscoveryMessage};
use crate::components::authentification::login::{LoginAction, LoginMessage};
use crate::components::authentification::verification::{VerificationAction, VerificationMessage};
use crate::components::home::{HomeAction, HomeMessage};
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

impl IcedWidget<AppMessage, Task<AppMessage>> for Screen {
    fn update(&mut self, message: AppMessage) -> Option<Task<AppMessage>> {
        match message {
            AppMessage::WindowFocus { focused } if let Screen::Home(home) = self => {
                home.set_frontend_focused(focused);
            }
            AppMessage::Home(msg) if let Screen::Home(home) = self => {
                if let Some(action) = home.update(msg) {
                    match action {
                        HomeAction::Run(task) => {
                            return Some(task.map(|_| AppMessage::DoNothing));
                        }
                        HomeAction::LoadTimeline(task) => {
                            return Some(task.map(|(room_id, message)| {
                                AppMessage::Home(HomeMessage::Timeline { room_id, message })
                            }));
                        }
                        HomeAction::LoadMediaTask(task) => {
                            return Some(
                                task.map(|media| AppMessage::Home(HomeMessage::MediaLoaded(media))),
                            );
                        }
                        HomeAction::TimelineScroll {
                            room_id,
                            direction,
                            task,
                        } => {
                            return Some(task.map(move |finished| {
                                let room_id = room_id.clone();
                                AppMessage::Home(HomeMessage::TimelineScrollFinished {
                                    room_id,
                                    direction,
                                    finished,
                                })
                            }));
                        }
                    }
                }
            }
            AppMessage::Discovery(msg) if let Screen::Discovery(discovery) = self => {
                match discovery.update(msg) {
                    Some(DiscoveryAction::Run(task)) => {
                        return Some(
                            task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res))),
                        );
                    }
                    Some(DiscoveryAction::ClientSelected(client)) => {
                        let (login, task) = Login::new(client);
                        *self = Screen::Login(login);
                        return Some(task.map(AppMessage::Login));
                    }
                    None => {}
                };
            }
            AppMessage::Login(msg) if let Screen::Login(login) = self => match login.update(msg) {
                Some(LoginAction::Run(task)) => {
                    return Some(task.map(|res| AppMessage::Login(LoginMessage::from(res))));
                }
                Some(LoginAction::BackToDiscovery(client)) => {
                    let (dis, task) = Discovery::from_client(client);
                    *self = Screen::Discovery(dis);
                    return Some(task.map(AppMessage::Discovery));
                }
                Some(LoginAction::LoginSuccess(state)) => {
                    *self = Screen::Verification(Verification::new(state));
                }
                None => {}
            },
            AppMessage::Verification(msg) if let Screen::Verification(verification) = self => {
                match verification.update(msg) {
                    None => {}
                    Some(VerificationAction::Run(task)) => {
                        return Some(
                            task.map(|res| {
                                AppMessage::Verification(VerificationMessage::from(res))
                            }),
                        );
                    }
                    Some(VerificationAction::Success(state)) => {
                        let (home, task) = Home::new(state);
                        *self = Screen::Home(Box::new(home));
                        return Some(task.map(AppMessage::Home));
                    }
                }
            }
            AppMessage::GoToLoading => *self = Screen::Loading,
            AppMessage::GoToLogin(client) => {
                let (login, task) = Login::new(client);
                *self = Screen::Login(login);
                return Some(task.map(AppMessage::Login));
            }
            AppMessage::Restored(result) => match result {
                RestoreResult::NeedsLogin(client) => {
                    let (login, task) = Login::new(client);
                    *self = Screen::Login(login);
                    return Some(task.map(AppMessage::Login));
                }
                RestoreResult::NoSession => {
                    let (discovery, task) = Discovery::new();
                    *self = Screen::Discovery(discovery);
                    return Some(
                        task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res))),
                    );
                }
                RestoreResult::Success(state) => {
                    let (home, task) = Home::new(*state);
                    *self = Screen::Home(Box::new(home));
                    return Some(task.map(AppMessage::Home));
                }
            },
            AppMessage::KeyboardEvent(event) => {
                if let Screen::Home(home) = self
                    && let Some(action) = home.update(HomeMessage::KeyboardEvent(event))
                {
                    match action {
                        HomeAction::Run(task) => {
                            return Some(task.map(|_| AppMessage::DoNothing));
                        }
                        HomeAction::LoadTimeline(task) => {
                            return Some(task.map(|(room_id, message)| {
                                AppMessage::Home(HomeMessage::Timeline { room_id, message })
                            }));
                        }
                        HomeAction::LoadMediaTask(task) => {
                            return Some(
                                task.map(|media| AppMessage::Home(HomeMessage::MediaLoaded(media))),
                            );
                        }
                        HomeAction::TimelineScroll {
                            room_id,
                            direction,
                            task,
                        } => {
                            return Some(task.map(move |finished| {
                                let room_id = room_id.clone();
                                AppMessage::Home(HomeMessage::TimelineScrollFinished {
                                    room_id,
                                    direction,
                                    finished,
                                })
                            }));
                        }
                    }
                }
            }
            _ => {}
        };

        None
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

impl Screen {
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
}

pub struct Root {
    screen: Screen,
    loading: shader::LoadingIndicator,
    theme: Theme,
    structure: Structure,

    id: Option<window::Id>,
    listener: Arc<Listener>,
}

impl Root {
    pub fn new(listener: Arc<Listener>) -> Self {
        Self {
            listener,
            id: None,
            screen: Screen::default(),
            loading: shader::LoadingIndicator::default(),
            theme: Theme::new(),
            structure: Structure::new(),
        }
    }
    pub fn update(&mut self, message: AppMessage) -> Task<AppMessage> {
        self.loading.transition_to(self.screen.state_index());

        if let AppMessage::TabPressed { shift } = message {
            return if shift {
                w::operation::focus_previous()
            } else {
                w::operation::focus_next()
            };
        }

        if let AppMessage::FocusRequest(_) = message {
            if let Some(id) = self.id {
                return window::gain_focus(id);
            } else {
                return window::open(window::Settings::default())
                    .1
                    .map(|_| AppMessage::DoNothing);
            }
        }

        if let AppMessage::Start(id) = message {
            self.id = Some(id)
        }

        if let AppMessage::WindowClosed(id) = message {
            if self.id.is_some_and(|i| i == id) {
                self.id = None
            }
        }

        self.screen.update(message).unwrap_or(Task::none())
    }

    pub fn title(&self, _: window::Id) -> String {
        if let Screen::Home(home) = &self.screen {
            return home.title();
        }
        "DePlace".to_string()
    }

    pub fn subscription(&self) -> Subscription<AppMessage> {
        Subscription::batch([
            iced::keyboard::listen().filter_map(|event| {
                if matches!(event, iced::keyboard::Event::KeyPressed { .. }) {
                    Some(AppMessage::KeyboardEvent(event))
                } else {
                    None
                }
            }),
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Window(window::Event::Focused) => {
                    Some(AppMessage::WindowFocus { focused: true })
                }
                iced::Event::Window(window::Event::Unfocused) => {
                    Some(AppMessage::WindowFocus { focused: false })
                }
                _ => None,
            }),
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
            window::close_events().map(AppMessage::WindowClosed),
            subscription::from_recipe(SocketListener {
                listener: self.listener.clone(),
            })
            .map(AppMessage::FocusRequest),
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
