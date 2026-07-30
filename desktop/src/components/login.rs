use std::sync::Arc;

use deplace_core::matrix_api::{LoginResult, login};
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, Render, Subscription, Window, div, prelude::*, red,
};
use gpui_component::{
    Disableable,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    v_flex,
};
use matrix_sdk::Client;

use crate::GenericState;

pub struct LoginView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    client: Client,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    recovery_key_input: Entity<InputState>,
    state: GenericState,
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
        let username_input = cx.new(|cx| InputState::new(window, cx).placeholder("Username"));
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .masked(true)
        });
        let recovery_key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Recovery key")
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

        Self {
            tokio_rt,
            client,
            username_input,
            password_input,
            recovery_key_input,
            _subscriptions,
            state: GenericState::Default,
        }
    }

    fn check_inputs(&mut self, cx: &Context<Self>) -> Option<(String, String, String)> {
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

        let client = self.client.clone();

        self.state = GenericState::Loading;

        cx.spawn(async move |this, cx| {
            let result = tokio_rt
                .spawn(async move { login(&client, username, password, recovery_key).await })
                .await
                .unwrap_or_default();

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
                    }
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
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loading = self.state.is_loading();
        let error = self.state.error();

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(gpui::px(320.))
                    .gap_3()
                    .child(Input::new(&self.username_input))
                    .child(Input::new(&self.password_input))
                    .child(Input::new(&self.recovery_key_input))
                    .when_some(error, |this, message| {
                        this.child(div().text_color(red()).child(message))
                    })
                    .child(
                        Button::new("discover-continue")
                            .label("Login")
                            .primary()
                            .w_full()
                            .loading(loading)
                            .disabled(loading)
                            .on_click(cx.listener(Self::on_login_click)),
                    ),
            )
    }
}
