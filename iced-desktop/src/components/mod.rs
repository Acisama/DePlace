use deplace_core::RestoreResult;
use iced::Font;
use iced::widget::text::Rich;
use iced::widget::{self as w, Text, rich_text, span, text};
use iced::{
    Border, Element,
    Length::Fill,
    Subscription, Task,
    widget::{Container, Shader, Stack},
    window,
};
use matrix_sdk::Room;

use crate::components::authentification::login::{LoginAction, LoginMessage};
use crate::components::authentification::verification::{
    Verification, VerificationAction, VerificationMessage,
};
use crate::things::{Colors, Theme};
use crate::{AppMessage, things::Structure};
use authentification::{
    discovery::{Discovery, DiscoveryAction, DiscoveryMessage},
    login::Login,
};
use home::Home;

pub(crate) mod authentification;
mod home;
pub(crate) mod shader;
mod sidebar;

pub enum GenericState<T: Clone> {
    Ready,
    Checking,
    Success(T),
    Error(String),
}

pub fn weighted_text<T>(text: &str, weight: iced::font::Weight) -> Rich<'_, (), T> {
    rich_text([span(text).font(Font {
        weight,
        ..Default::default()
    })])
}

impl<T: Clone> GenericState<T> {
    pub fn success(&self) -> Option<T> {
        match self {
            GenericState::Success(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn ready(&self) -> bool {
        matches!(self, GenericState::Ready)
    }

    pub fn text<'a, V>(
        &'a self,
        success: &'a str,
        ready: &'a str,
        colors: &Colors,
        structure: &Structure,
    ) -> Rich<'a, (), V> {
        let (text, color) = match self {
            GenericState::Ready => (ready, colors.success),
            GenericState::Success(_) => (success, colors.success),
            GenericState::Error(e) => (e.as_str(), colors.error),
            GenericState::Checking => ("Checking...", colors.offline),
        };

        weighted_text(text, iced::font::Weight::Semibold)
            .color(color)
            .size(structure.font_size)
    }
}

#[derive(Default)]
enum Screen {
    #[default]
    Loading,
    Discovery(Discovery),
    Login(Login),
    Verification(Verification),
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
                        *self = Screen::Home(Home::new(state));
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
                RestoreResult::NoSession | RestoreResult::Success(_) => {
                    let (discovery, task) = Discovery::new();
                    *self = Screen::Discovery(discovery);
                    return task.map(|res| AppMessage::Discovery(DiscoveryMessage::from(res)));
                } // RestoreResult::Success(state) => Screen::Home(Home::new(*state)),
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

    fn view<'a>(&'a self, theme: &'a Theme, structure: &'a Structure) -> Element<'a, AppMessage> {
        match self {
            Screen::Loading => "loading".into(),
            Screen::Discovery(discovery) => {
                discovery.view(theme, structure).map(AppMessage::Discovery)
            }
            Screen::Login(login) => login.view(theme, structure).map(AppMessage::Login),
            Screen::Verification(verification) => verification
                .view(theme, structure)
                .map(AppMessage::Verification),
            Screen::Home { .. } => "home".into(),
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

        Stack::new()
            .push(background)
            .push(self.screen.view(&self.theme, &self.structure))
            .into()
    }
}

pub fn floating_tile<'a, 'b, T>(
    theme: &'a Theme,
    structure: &'a Structure,
    content: impl Into<Element<'b, T>>,
) -> Container<'b, T>
where
    'a: 'b,
{
    use w::container::Style;
    w::container(content).style(move |_theme| Style {
        background: Some(theme.background.into()),
        border: Border {
            color: theme.border,
            width: structure.border_thickness,
            radius: structure.outer_border_radius.into(),
        },
        ..Style::default()
    })
}

pub fn text_input<'a, 'b, T>(
    placeholder: &'a str,
    value: &'a str,
    theme: &'a Theme,
    structure: &'a Structure,
) -> w::text_input::TextInput<'b, T>
where
    'a: 'b,
    T: Clone,
{
    use w::text_input::{Status, Style};

    w::text_input(placeholder, value)
        .size(structure.font_size)
        .padding(structure.small_gap)
        .style(move |_theme, status| Style {
            background: theme.background.into(),
            border: Border {
                color: if matches!(status, Status::Focused { .. }) {
                    theme.accent
                } else {
                    theme.border
                },
                width: structure.border_thickness,
                radius: structure.inner_border_radius.into(),
            },
            placeholder: theme.text.muted,
            icon: theme.text.muted,
            selection: theme.text.muted,
            value: theme.text.normal,
        })
}
