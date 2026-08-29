use std::future::Ready;

use deplace_core::{
    matrix_api::{LoginResult, login},
    state::AppState,
};
use iced::{
    Border, Element,
    Length::Fill,
    Task,
    widget::{self as w, Id, Space, button::Status, column, container, text},
};
use matrix_sdk::Client;

use crate::{
    components::{GenericState, floating_tile, text_input, weighted_text},
    things::{Structure, Theme},
};

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
    None,
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

    pub fn update(&mut self, message: LoginMessage) -> LoginAction {
        match message {
            LoginMessage::UsernameChanged(username) => {
                self.username = username;
                self.check_inputs();
                LoginAction::None
            }
            LoginMessage::PasswordChanged(password) => {
                self.password = password;
                self.check_inputs();
                LoginAction::None
            }
            LoginMessage::Checking => {
                self.state = GenericState::Checking;
                LoginAction::None
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

                LoginAction::Run(task)
            }
            LoginMessage::LoginFailed(e) => {
                self.state = GenericState::Error(e);
                LoginAction::None
            }
            LoginMessage::LoginSuccess(state) => {
                self.state = GenericState::Success(state.clone());
                LoginAction::LoginSuccess(state)
            }
            LoginMessage::BackToDiscovery(client) => LoginAction::BackToDiscovery(client),
        }
    }

    pub fn view<'a>(
        &'a self,
        theme: &'a Theme,
        structure: &'a Structure,
    ) -> Element<'a, LoginMessage> {
        let username_input: iced::widget::TextInput<'_, LoginMessage> =
            text_input("luke", &self.username, theme, structure)
                .id(USERNAME_ID)
                .width(Fill)
                .on_input(LoginMessage::UsernameChanged)
                .on_submit(LoginMessage::Submit);

        let password_input: iced::widget::TextInput<'_, LoginMessage> =
            text_input("•••••••••••", &self.password, theme, structure)
                .secure(true)
                .width(Fill)
                .on_input(LoginMessage::PasswordChanged)
                .on_submit(LoginMessage::Submit);

        let status: iced::widget::text::Rich<'_, (), LoginMessage> = self.state.text(
            "Successfully logged in",
            "Ready to log in",
            &theme.colors,
            structure,
        );

        let back_button: iced::widget::text::Rich<'_, (), LoginMessage> =
            w::rich_text([w::span("← back").link(())])
                .size(structure.font_size)
                .color(theme.text.dim)
                .on_link_click({
                    let client = self.client.clone();
                    move |_| LoginMessage::BackToDiscovery(client.clone())
                });

        let login_button = w::button(text("Log in").width(Fill).center())
            .width(Fill)
            .style(move |_, status| w::button::Style {
                background: Some(match status {
                    Status::Disabled => theme.colors.muted.into(),
                    Status::Active => theme.accent.into(),
                    _ => theme.accent.scale_alpha(0.5).into(),
                }),
                text_color: theme.background,
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
            column![
                back_button,
                weighted_text("Login", iced::font::Weight::ExtraBold)
                    .size(structure.large_font_size)
                    .color(theme.accent)
                    .width(Fill)
                    .center(),
                column![
                    text("Username")
                        .size(structure.font_size)
                        .color(theme.text.dim),
                    Space::new().height(structure.small_gap),
                    username_input,
                ],
                column![
                    text("Password")
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

        container(tile).center(Fill).into()
    }
}
