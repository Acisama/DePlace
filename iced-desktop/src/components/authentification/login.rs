use crate::common::*;
use std::future::Ready;

use deplace_core::matrix_api::{LoginResult, login};
use iced::widget::{Id, button::Status};
// use iced::{
//     Border, Element,
//     Length::Fill,
//     Task,
//     widget::{self as w, Id, Space, button::Status, column, container, text},
// };
// use matrix_sdk::Client;

use crate::components::{GenericState, floating_tile, text_input, weighted_text};

#[derive(Debug, Clone)]
pub enum LoginMessage {
    UsernameChanged(String),
    PasswordChanged(String),
    Checking,
    Submit,
    LoginFailed(String),
    LoginSuccess(AppState),
    BackToDiscovery(Client),
}

impl From<LoginResult> for LoginMessage {
    fn from(result: LoginResult) -> Self {
        match result {
            LoginResult::Error(error) => LoginMessage::LoginFailed(error),
            LoginResult::InvalidCredentials => {
                LoginMessage::LoginFailed("Invalid credentials".to_string())
            }
            LoginResult::Success(state) => LoginMessage::LoginSuccess(state),
        }
    }
}

#[derive(Debug)]
pub enum LoginAction {
    Run(Task<LoginResult>),
    BackToDiscovery(Client),
    LoginSuccess(AppState),
}

pub struct Login {
    client: matrix_sdk::Client,
    username: String,
    password: String,
    state: GenericState<AppState>,

    current_check: Option<iced::task::Handle>,
}

impl Hash for Login {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.username.hash(state);
        self.password.hash(state);
        self.state.hash(state);
    }
}

type DummyFut = Ready<matrix_sdk::Result<()>>;
type DummyFn = fn(String) -> DummyFut;

const USERNAME_ID: Id = Id::new("login-username");

impl Login {
    pub fn new(client: matrix_sdk::Client) -> (Self, Task<LoginMessage>) {
        let mut login = Self {
            client,
            username: "".to_string(),
            password: "".to_string(),
            state: GenericState::Checking,
            current_check: None,
        };
        login.check_inputs();

        (login, w::operation::focus(USERNAME_ID))
    }

    pub fn check_inputs(&mut self) {
        if self.username.is_empty() {
            self.state = GenericState::Error("Username must not be empty".into());
        } else if self.password.is_empty() {
            self.state = GenericState::Error("Password must not be empty".into());
        } else {
            self.state = GenericState::Ready;
        }
    }
}

impl IcedWidget<LoginMessage, LoginAction> for Login {
    fn update(&mut self, message: LoginMessage) -> Option<LoginAction> {
        match message {
            LoginMessage::UsernameChanged(username) => {
                self.username = username;
                self.check_inputs();
                None
            }
            LoginMessage::PasswordChanged(password) => {
                self.password = password;
                self.check_inputs();
                None
            }
            LoginMessage::Checking => {
                self.state = GenericState::Checking;
                None
            }
            LoginMessage::Submit => {
                self.state = GenericState::Checking;

                let old_client = self.client.clone();
                let username = self.username.clone();
                let password = self.password.clone();

                let (task, handle) = Task::future(login(deplace_core::matrix_api::LoginMethod::<
                    DummyFn,
                    DummyFut,
                >::Credentials {
                    old_client,
                    username,
                    password,
                }))
                .abortable();
                self.current_check = Some(handle.abort_on_drop());

                Some(LoginAction::Run(task))
            }
            LoginMessage::LoginFailed(e) => {
                self.state = GenericState::Error(e);
                None
            }
            LoginMessage::LoginSuccess(state) => {
                self.state = GenericState::Success(state.clone());
                Some(LoginAction::LoginSuccess(state))
            }
            LoginMessage::BackToDiscovery(client) => Some(LoginAction::BackToDiscovery(client)),
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, LoginMessage> {
        let username_input: iced::widget::TextInput<'static, LoginMessage> =
            text_input("luke", &self.username, theme, structure)
                .id(USERNAME_ID)
                .width(Fill)
                .on_input(LoginMessage::UsernameChanged)
                .on_submit(LoginMessage::Submit);

        let password_input: iced::widget::TextInput<'static, LoginMessage> =
            text_input("•••••••••••", &self.password, theme, structure)
                .secure(true)
                .width(Fill)
                .on_input(LoginMessage::PasswordChanged)
                .on_submit(LoginMessage::Submit);

        let status: iced::widget::text::Rich<'static, (), LoginMessage> = self.state.text(
            "Successfully logged in",
            "Ready to log in",
            &theme.colors,
            &structure,
        );

        let back_button: iced::widget::text::Rich<'static, (), LoginMessage> =
            w::rich_text([w::span("← back").link(())])
                .size(structure.font_size)
                .color(theme.text.dim)
                .on_link_click({
                    let client = self.client.clone();
                    move |_| LoginMessage::BackToDiscovery(client.clone())
                });

        let login_button = w::button(w::text("Log in").width(Fill).center())
            .width(Fill)
            .style(move |_, status| w::button::Style {
                background: Some(match status {
                    Status::Disabled => theme.colors.muted.into(),
                    Status::Active => theme.accent.into(),
                    _ => theme.accent.scale_alpha(0.5).into(),
                }),
                text_color: theme.background.into(),
                border: Border {
                    radius: structure.semi_border_radius().into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press_maybe(self.state.ready().then_some(LoginMessage::Submit));

        let tile = floating_tile(
            theme,
            structure,
            w::column![
                back_button,
                weighted_text("Login", iced::font::Weight::ExtraBold)
                    .size(structure.large_font_size)
                    .color(theme.accent)
                    .width(Fill)
                    .center(),
                w::column![
                    w::text("Username")
                        .size(structure.font_size)
                        .color(theme.text.dim),
                    Space::new().height(structure.small_gap),
                    username_input,
                ],
                w::column![
                    w::text("Password")
                        .size(structure.font_size)
                        .color(theme.text.dim),
                    Space::new().height(structure.small_gap),
                    password_input,
                ],
                status,
                login_button
            ]
            .width(structure.authentification.width)
            .spacing(structure.gap)
            .padding(structure.gap),
        );

        w::container(tile).center(Fill).into()
    }
}
