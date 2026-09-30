use deplace_core::RestoreResult;
use deplace_core::state::ImportantPaths;
use deplace_core::structure::Structure;
use deplace_core::theme::Theme;
use iced::advanced::subscription;
use iced::widget::Shader;
use iced::window;
use interprocess::local_socket::Listener;

use crate::{SocketListener, common::*};

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
    Discovery(Box<Discovery>),
    Login(Box<Login>),
    Verification(Box<Verification>),
    Home(Box<Home>),
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

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> Element<'static, AppMessage> {
        match self {
            Screen::Loading => Space::new().into(),
            Screen::Discovery(discovery) => discovery
                .view(theme, structure, help_state)
                .map(AppMessage::Discovery),
            Screen::Login(login) => login
                .view(theme, structure, help_state)
                .map(AppMessage::Login),
            Screen::Verification(verification) => verification
                .view(theme, structure, help_state)
                .map(AppMessage::Verification),
            Screen::Home(home) => home
                .view(theme, structure, help_state)
                .map(AppMessage::Home),
        }
    }
}

pub struct Root {
    paths: ImportantPaths,
    screen: Screen,
    loading: shader::LoadingIndicator,
    theme: Theme,
    structure: Structure,

    id: Option<window::Id>,
    listener: Arc<Listener>,
}

impl Root {
    pub fn new(paths: ImportantPaths, listener: Arc<Listener>) -> Self {
        Self {
            listener,
            id: None,
            screen: Screen::default(),
            loading: shader::LoadingIndicator::default(),
            theme: Theme::new(paths.theme_file.clone()),
            structure: Structure::new(paths.structure_file.clone()),
            paths,
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
            tracing::debug!("Received focus request.");
            if let Some(id) = self.id {
                tracing::debug!("Window is open. Focusing Window.");
                return window::gain_focus(id);
            } else {
                tracing::debug!("Window is closed. Opening Window.");
                return window::open(window::Settings::default())
                    .1
                    .map(|_| AppMessage::DoNothing);
            }
        }

        if let AppMessage::WindowOpened(id) = message {
            tracing::debug!("Window opened.");
            if self.id.is_none() {
                tracing::debug!("No window was open.");
                self.id = Some(id)
            }
        }

        if let AppMessage::Start(id) = message {
            self.id = Some(id)
        }

        if let AppMessage::WindowClosed(id) = message
            && self.id.is_some_and(|i| i == id)
        {
            tracing::info!("Closing window");
            self.id = None
        }

        self.update_screen(message).unwrap_or(Task::none())
    }

    fn update_screen(&mut self, message: AppMessage) -> Option<Task<AppMessage>> {
        match message {
            AppMessage::WindowFocus { focused } if let Screen::Home(home) = &mut self.screen => {
                home.set_frontend_focused(focused);
                None
            }
            AppMessage::Home(msg) if let Screen::Home(home) = &mut self.screen => {
                match home.update(msg)? {
                    HomeAction::Run(task) => Some(task.map(|_| AppMessage::DoNothing)),
                    HomeAction::Perform(task) => Some(task.map(AppMessage::Home)),
                    HomeAction::LoadTimeline(task) => Some(task.map(|(room_id, message)| {
                        AppMessage::Home(HomeMessage::Chat { room_id, message })
                    })),
                    HomeAction::LoadMediaTask(task) => {
                        Some(task.map(|media| AppMessage::Home(HomeMessage::MediaLoaded(media))))
                    }
                    HomeAction::TimelineScroll {
                        room_id,
                        direction,
                        task,
                    } => Some(task.map(move |finished| {
                        let room_id = room_id.clone();
                        AppMessage::Home(HomeMessage::TimelineScrollFinished {
                            room_id,
                            direction,
                            finished,
                        })
                    })),
                }
            }
            AppMessage::Discovery(msg) if let Screen::Discovery(discovery) = &mut self.screen => {
                match discovery.update(msg)? {
                    DiscoveryAction::Run(task) => {
                        Some(task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res))))
                    }
                    DiscoveryAction::ClientSelected(client) => {
                        let (login, task) = Login::new(client, self.paths.clone());
                        self.screen = Screen::Login(Box::new(login));
                        Some(task.map(AppMessage::Login))
                    }
                }
            }
            AppMessage::Login(msg) if let Screen::Login(login) = &mut self.screen => {
                match login.update(msg)? {
                    LoginAction::Run(task) => {
                        Some(task.map(|res| AppMessage::Login(LoginMessage::from(res))))
                    }
                    LoginAction::BackToDiscovery(client) => {
                        let (discovery, task) = Discovery::from_client(client);
                        self.screen = Screen::Discovery(Box::new(discovery));
                        Some(task.map(AppMessage::Discovery))
                    }
                    LoginAction::LoginSuccess(state) => {
                        self.screen = Screen::Verification(Box::new(Verification::new(state)));
                        None
                    }
                }
            }
            AppMessage::Verification(msg)
                if let Screen::Verification(verification) = &mut self.screen =>
            {
                match verification.update(msg)? {
                    VerificationAction::Run(task) => Some(
                        task.map(|res| AppMessage::Verification(VerificationMessage::from(res))),
                    ),
                    VerificationAction::Success(state) => {
                        let (home, task) = Home::new(state);
                        self.screen = Screen::Home(Box::new(home));
                        Some(task.map(AppMessage::Home))
                    }
                }
            }
            AppMessage::GoToLoading => {
                self.screen = Screen::Loading;
                None
            }
            AppMessage::GoToLogin(client) => {
                let (login, task) = Login::new(client, self.paths.clone());
                self.screen = Screen::Login(Box::new(login));
                Some(task.map(AppMessage::Login))
            }
            AppMessage::Restored(result) => match result {
                RestoreResult::NeedsLogin(client) => {
                    let (login, task) = Login::new(client, self.paths.clone());
                    self.screen = Screen::Login(Box::new(login));
                    Some(task.map(AppMessage::Login))
                }
                RestoreResult::NoSession => {
                    let (discovery, task) = Discovery::new();
                    self.screen = Screen::Discovery(Box::new(discovery));
                    Some(task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res))))
                }
                RestoreResult::Success(state) => {
                    let (home, task) = Home::new(*state);
                    self.screen = Screen::Home(Box::new(home));
                    Some(task.map(AppMessage::Home))
                }
            },
            AppMessage::KeyboardEvent(event) => {
                if let Screen::Home(home) = &mut self.screen {
                    match home.update(HomeMessage::KeyboardEvent(event))? {
                        HomeAction::Run(task) => Some(task.map(|_| AppMessage::DoNothing)),
                        HomeAction::Perform(task) => Some(task.map(AppMessage::Home)),
                        HomeAction::LoadTimeline(task) => Some(task.map(|(room_id, message)| {
                            AppMessage::Home(HomeMessage::Chat { room_id, message })
                        })),
                        HomeAction::LoadMediaTask(task) => Some(
                            task.map(|media| AppMessage::Home(HomeMessage::MediaLoaded(media))),
                        ),
                        HomeAction::TimelineScroll {
                            room_id,
                            direction,
                            task,
                        } => Some(task.map(move |finished| {
                            let room_id = room_id.clone();
                            AppMessage::Home(HomeMessage::TimelineScrollFinished {
                                room_id,
                                direction,
                                finished,
                            })
                        })),
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
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
            window::open_events().map(AppMessage::WindowOpened),
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
                screen.view(theme, structure, HelpState::default())
            }))
            .into()
    }
}
