use deplace_core::{RestoreResult, state::ImportantPaths};
use iced::{advanced::subscription, widget::Shader, window};
use interprocess::local_socket::Listener;
use tray_icon::{Icon, TrayIcon};

use crate::{SocketListener, TrayAction, TrayIconClickListener, TrayMenuListener, common::*};

use crate::components::{MapElementExt, shader};
use crate::{
    AppMessage,
    modules::{
        authentification::{
            discovery::{Discovery, DiscoveryAction, DiscoveryMessage},
            login::{Login, LoginAction, LoginMessage},
            verification::{Verification, VerificationAction, VerificationMessage},
        },
        home::{Home, HomeAction, HomeMessage},
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

    fn view(&self) -> Element<'static, AppMessage> {
        match self {
            Screen::Loading => Space::new().boxed(),
            Screen::Discovery(discovery) => discovery.view().map_el(AppMessage::Discovery),
            Screen::Login(login) => login.view().map_el(AppMessage::Login),
            Screen::Verification(verification) => {
                verification.view().map_el(AppMessage::Verification)
            }
            Screen::Home(home) => home.view().map_el(|m| AppMessage::Home(Box::new(m))),
        }
    }
}

pub struct Root {
    paths: ImportantPaths,
    screen: Screen,
    loading: shader::LoadingIndicator,

    id: Option<window::Id>,
    listener: Arc<Listener>,

    tray: TrayIcon,
    tray_icon_normal: Icon,
    tray_icon_mic_active: Icon,
    /// Whether the tray icon is currently showing the mic-active badge, so
    /// we only call `set_icon` on actual transitions.
    tray_mic_active: bool,
}

impl Root {
    pub fn new(
        paths: ImportantPaths,
        listener: Arc<Listener>,
        tray: TrayIcon,
        tray_icon_normal: Icon,
        tray_icon_mic_active: Icon,
    ) -> Self {
        Self {
            listener,
            id: None,
            screen: Screen::default(),
            loading: shader::LoadingIndicator::default(),
            paths,
            tray,
            tray_icon_normal,
            tray_icon_mic_active,
            tray_mic_active: false,
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

        if let AppMessage::FocusRequest(_) | AppMessage::Tray(TrayAction::Open) = message {
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

        if let AppMessage::Tray(TrayAction::CloseWindow) = message {
            tracing::debug!("Received tray close request.");
            if let Some(id) = self.id {
                return window::close(id);
            }
        }

        if let AppMessage::Tray(TrayAction::Exit) = message {
            tracing::debug!("Received tray exit request.");
            // Every ongoing call is left before the app exits, else our
            // call memberships stay behind in the rooms and ghost us
            // into calls we are not in anymore.
            if let Screen::Home(home) = &self.screen {
                let state = home.state().clone();
                return Task::future(async move {
                    state.call_registry().leave_all().await;
                })
                .map(|()| AppMessage::Exit);
            }
            return iced::exit();
        }

        if let AppMessage::Exit = message {
            return iced::exit();
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
                if focused {
                    // Let the open chat re-check whether it counts as read
                    // now that the window is focused and it will then set
                    // read markers based on this
                    self.update_screen(AppMessage::Home(Box::new(HomeMessage::WindowFocused)))
                } else {
                    None
                }
            }
            AppMessage::Home(msg) if let Screen::Home(home) = &mut self.screen => {
                let is_call_tick = matches!(*msg, HomeMessage::CallTick);
                let action = home.update(*msg);

                if is_call_tick {
                    let speaking = home.state().call_registry().any_own_speaking();
                    if speaking != self.tray_mic_active {
                        self.tray_mic_active = speaking;
                        let icon = if speaking {
                            self.tray_icon_mic_active.clone()
                        } else {
                            self.tray_icon_normal.clone()
                        };
                        if let Err(e) = self.tray.set_icon(Some(icon)) {
                            tracing::warn!("Failed to update tray icon: {e}");
                        }
                    }
                }

                match action? {
                    HomeAction::Run(task) => Some(task.map(|_| AppMessage::DoNothing)),
                    HomeAction::Perform(task) => Some(task.map(|m| AppMessage::Home(Box::new(m)))),
                    HomeAction::LoadTimeline(task) => Some(task.map(|(room_id, message)| {
                        AppMessage::Home(Box::new(HomeMessage::Chat { room_id, message }))
                    })),
                    HomeAction::TimelineScroll {
                        room_id,
                        direction,
                        task,
                    } => Some(task.map(move |finished| {
                        let room_id = room_id.clone();
                        AppMessage::Home(Box::new(HomeMessage::TimelineScrollFinished {
                            room_id,
                            direction,
                            finished,
                        }))
                    })),
                    HomeAction::SetFullscreen(fullscreen) => self.id.map(|id| {
                        window::set_mode(
                            id,
                            if fullscreen {
                                window::Mode::Fullscreen
                            } else {
                                window::Mode::Windowed
                            },
                        )
                    }),
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
                        Some(task.map(|m| AppMessage::Home(Box::new(m))))
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
                    Some(task.map(|m| AppMessage::Home(Box::new(m))))
                }
            },
            AppMessage::KeyboardEvent(event) => {
                if let Screen::Home(home) = &mut self.screen {
                    match home.update(HomeMessage::KeyboardEvent(event))? {
                        HomeAction::Run(task) => Some(task.map(|_| AppMessage::DoNothing)),
                        HomeAction::Perform(task) => {
                            Some(task.map(|m| AppMessage::Home(Box::new(m))))
                        }
                        HomeAction::LoadTimeline(task) => Some(task.map(|(room_id, message)| {
                            AppMessage::Home(Box::new(HomeMessage::Chat { room_id, message }))
                        })),
                        HomeAction::TimelineScroll {
                            room_id,
                            direction,
                            task,
                        } => Some(task.map(move |finished| {
                            let room_id = room_id.clone();
                            AppMessage::Home(Box::new(HomeMessage::TimelineScrollFinished {
                                room_id,
                                direction,
                                finished,
                            }))
                        })),
                        HomeAction::SetFullscreen(fullscreen) => self.id.map(|id| {
                            window::set_mode(
                                id,
                                if fullscreen {
                                    window::Mode::Fullscreen
                                } else {
                                    window::Mode::Windowed
                                },
                            )
                        }),
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
            subscription::from_recipe(TrayMenuListener).map(AppMessage::Tray),
            subscription::from_recipe(TrayIconClickListener).map(AppMessage::Tray),
        ])
    }

    pub fn view(&self, _window: window::Id) -> Element<'_, AppMessage> {
        let background = Shader::new(self.loading).width(Fill).height(Fill);

        w::stack![background, lazy(&self.screen, |screen| screen.view()),].boxed()
    }
}
