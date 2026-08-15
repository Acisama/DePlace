use std::sync::Arc;

use deplace_core::matrix_api::{LoginResult, login};
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, Render, Role, Subscription, Window, div, prelude::*,
};
use gpui_component::{
    Disableable, StyledExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
};
use matrix_sdk::Client;

use crate::{
    GenericState,
    components::{floating_tile, input},
    theme::DeplaceThings,
};

pub struct LoginView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    client: Client,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    recovery_key_input: Entity<InputState>,
    state: GenericState,
    current_request: Option<tokio::task::AbortHandle>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LoginResult> for LoginView {}

impl LoginView {
    pub fn new(
        tokio_rt: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
        client: Client,
    ) -> Self {
        let username_input = cx.new(|cx| InputState::new(window, cx).placeholder("luke"));
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("••••••••")
                .masked(true)
        });
        let recovery_key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Es9o xxxx xxxx...")
                .masked(true)
        });

        let inputs = [&username_input, &password_input, &recovery_key_input];
        let input_subscriptions = inputs.iter().map(|input| {
            cx.subscribe_in(input, window, |this, _, event: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.perform_login(cx);
                } else {
                    this.check_inputs(cx);
                }
            })
        });
        let _subscriptions = input_subscriptions.collect();

        let mut view = Self {
            tokio_rt,
            client,
            username_input,
            password_input,
            recovery_key_input,
            current_request: None,
            _subscriptions,
            state: GenericState::Default,
        };
        view.check_inputs(cx);
        view
    }

    fn check_inputs(&mut self, cx: &mut Context<Self>) -> Option<(String, String, String)> {
        let username = self.username_input.read(cx).value().to_string();
        if username.is_empty() {
            self.state = GenericState::Error("Username is required".to_string());
            return None;
        }

        let password = self.password_input.read(cx).value().to_string();
        if password.is_empty() {
            self.state = GenericState::Error("Password is required".to_string());
            return None;
        }

        let key = self.recovery_key_input.read(cx).value().to_string();
        if key.is_empty() {
            self.state = GenericState::Error("Recovery key is required".to_string());
            return None;
        }

        let parts: Vec<&str> = key.split_whitespace().collect();
        if parts.len() != 12 {
            self.state = GenericState::Error("Recovery key must contain 12 words".to_string());
            return None;
        }
        for part in parts {
            if part.len() != 4 {
                self.state =
                    GenericState::Error("Each recovery key word must be 4 characters".to_string());
                return None;
            }
        }

        self.state = GenericState::Default;
        cx.notify();
        Some((username, password, key))
    }

    fn perform_login(&mut self, cx: &mut Context<Self>) {
        let tokio_rt = Arc::clone(&self.tokio_rt);

        if self.state.is_loading() {
            return;
        }

        let Some((username, password, recovery_key)) = self.check_inputs(cx) else {
            return;
        };

        if let Some(handle) = self.current_request.take() {
            handle.abort();
        }

        let client = self.client.clone();

        self.state = GenericState::Loading;
        cx.notify();

        let task =
            tokio_rt.spawn(async move { login(&client, username, password, recovery_key).await });
        self.current_request = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_default();

            cx.update(|cx| {
                let _ = this.update(cx, |view, cx| {
                    match &result {
                        LoginResult::Success(_) => {
                            view.state = GenericState::Success;
                        }
                        LoginResult::InvalidCredentials => {
                            view.state = GenericState::Error("Invalid credentials".to_string());
                        }
                        LoginResult::Error(err) => {
                            view.state = GenericState::Error(err.clone());
                        }
                        LoginResult::BackToDiscovery => {
                            view.state = GenericState::Default;
                        }
                    }
                    view.current_request = None;
                    cx.emit(result);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn on_login_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.perform_login(cx);
    }

    fn on_back_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(LoginResult::BackToDiscovery);
    }
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        let disabled = self.state.is_disabled();

        let (message, color) = match &self.state {
            GenericState::Default => ("All inputs look good".to_string(), theme.colors.success),
            GenericState::Loading | GenericState::Disabled => {
                ("Logging in...".to_string(), theme.colors.muted)
            }
            GenericState::Error(err) => (err.clone(), theme.colors.error),
            GenericState::Success => ("Logged in successfully".to_string(), theme.colors.success),
        };

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                floating_tile(theme, structure)
                    .paddings(structure.gap)
                    .flex_col()
                    .gap(structure.gap * 2)
                    .w(gpui::px(360.0))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .child(
                                div()
                                    .absolute()
                                    .left(structure.gap)
                                    .top(structure.gap)
                                    .child(
                                        h_flex()
                                            .id("login-back")
                                            .items_center()
                                            .gap_1()
                                            .cursor_pointer()
                                            .text_color(theme.text.dim)
                                            .text_xs()
                                            .font_bold()
                                            .hover(|style| style.text_decoration_1())
                                            .child("⟵ back")
                                            .on_click(cx.listener(Self::on_back_click)),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .text_center()
                                    .text_2xl()
                                    .font_extrabold()
                                    .text_color(theme.accent)
                                    .child("Login"),
                            ),
                    )
                    .child(
                        div()
                            .flex_col()
                            .flex()
                            .gap(structure.small_gap)
                            .child("Username")
                            .text_color(theme.text.dim)
                            .child(input(&self.username_input, window, cx, Role::TextInput)),
                    )
                    .child(
                        div()
                            .flex_col()
                            .flex()
                            .gap(structure.small_gap)
                            .child("Password")
                            .text_color(theme.text.dim)
                            .child(input(&self.password_input, window, cx, Role::TextInput)),
                    )
                    .child(
                        div()
                            .flex_col()
                            .flex()
                            .gap(structure.small_gap)
                            .child("Recovery Key")
                            .text_color(theme.text.dim)
                            .child(input(&self.recovery_key_input, window, cx, Role::TextInput)),
                    )
                    .child(
                        div()
                            .flex_col()
                            .flex()
                            .gap(structure.small_gap)
                            .child(div().child(message).text_color(color))
                            .child({
                                let bg = if disabled {
                                    theme.colors.muted
                                } else {
                                    theme.accent
                                };
                                let variant =
                                    ButtonCustomVariant::new(cx).color(bg).hover(bg).active(bg);

                                Button::new("login-submit")
                                    .label("Log in")
                                    .custom(variant)
                                    .w_full()
                                    .disabled(disabled)
                                    .on_click(cx.listener(Self::on_login_click))
                            }),
                    ),
            )
    }
}
