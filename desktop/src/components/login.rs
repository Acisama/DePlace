use std::{future::Ready, sync::Arc};

use deplace_core::matrix_api::{LoginMethod, LoginResult, login};
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, Render, Role, Subscription, Window, div, prelude::*,
};
use gpui_component::{
    Disableable, StyledExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
};
use matrix_sdk::{Client, ruma::api::client::session::get_login_types::v3::LoginType};

use crate::{
    GenericState,
    components::{floating_tile, input},
    things::DeplaceThings,
};

pub struct LoginView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    client: Client,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    state: GenericState,
    has_sso: bool,
    sso_only: bool,
    login_types_loaded: bool,
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

        let inputs = [&username_input, &password_input];
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
            current_request: None,
            _subscriptions,
            state: GenericState::Default,
            has_sso: false,
            sso_only: false,
            login_types_loaded: false,
        };

        view.fetch_login_types(cx);
        view.check_inputs(cx);
        view
    }

    fn fetch_login_types(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        let tokio_rt = Arc::clone(&self.tokio_rt);

        let task = tokio_rt.spawn(async move { client.matrix_auth().get_login_types().await });

        cx.spawn(async move |this, cx| {
            let res = task.await;

            if let Err(e) = this.update(cx, |view, cx| {
                if let Ok(Ok(response)) = res {
                    for flow in response.flows {
                        if let LoginType::Sso(sso) = flow {
                            view.has_sso = true;
                            if sso.oauth_aware_preferred {
                                view.sso_only = true;
                            }
                        }
                    }
                }
                view.login_types_loaded = true;
                cx.notify();
            }) {
                tracing::error!("Failed to fetch login types: {e:?}");
            }
        })
        .detach();
    }

    fn check_inputs(&mut self, cx: &mut Context<Self>) -> Option<(String, String)> {
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

        self.state = GenericState::Default;
        cx.notify();
        Some((username, password))
    }

    fn perform_login(&mut self, cx: &mut Context<Self>) {
        let tokio_rt = Arc::clone(&self.tokio_rt);

        if self.state.is_loading() {
            return;
        }

        let Some((username, password)) = self.check_inputs(cx) else {
            return;
        };

        if let Some(handle) = self.current_request.take() {
            handle.abort();
        }

        let client = self.client.clone();

        self.state = GenericState::Loading;
        cx.notify();

        type DummyFut = Ready<matrix_sdk::Result<()>>;
        type DummyFn = fn(String) -> DummyFut;

        let task = tokio_rt.spawn(async move {
            login(LoginMethod::<DummyFn, DummyFut>::Credentials {
                old_client: client,
                username,
                password,
            })
            .await
        });
        self.current_request = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            let result = task.await.unwrap_or_default();

            cx.update(|cx| {
                if let Err(e) = this.update(cx, |view, cx| {
                    view.current_request = None;

                    match &result {
                        LoginResult::ValidCredentials(_) => {
                            view.state = GenericState::Success;
                            cx.emit(result);
                        }
                        LoginResult::InvalidCredentials => {
                            view.state = GenericState::Error("Invalid credentials".to_string());
                            cx.emit(result);
                        }
                        LoginResult::Error(err) => {
                            view.state = GenericState::Error(err.clone());
                            cx.emit(result);
                        }
                        LoginResult::BackToDiscovery => {
                            view.state = GenericState::Default;
                            cx.emit(result);
                        }
                    }
                    cx.notify();
                }) {
                    tracing::error!("Failed to update login state: {e:?}");
                }
            });
        })
        .detach();
    }

    fn on_login_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.perform_login(cx);
    }

    fn on_sso_login_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.state.is_loading() {
            return;
        }

        if let Some(handle) = self.current_request.take() {
            handle.abort();
        }

        let client = self.client.clone();
        let tokio_rt = Arc::clone(&self.tokio_rt);

        self.state = GenericState::Loading;
        cx.notify();

        let task = tokio_rt.spawn(async move {
            login(LoginMethod::Sso {
                authenticated_client: client,
                url_handler: |url| async move {
                    webbrowser::open(&url).map_err(|e| matrix_sdk::Error::UnknownError(Box::new(e)))
                },
            })
            .await
        });

        self.current_request = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            let result = task.await;

            cx.update(|cx| {
                if let Err(e) = this.update(cx, |view, cx| {
                    view.current_request = None;

                    match result {
                        Ok(login_result) => match &login_result {
                            LoginResult::ValidCredentials(_) => {
                                view.state = GenericState::Success;
                                cx.emit(login_result);
                            }
                            LoginResult::InvalidCredentials => {
                                view.state = GenericState::Error("Invalid credentials".to_string());
                                cx.emit(login_result);
                            }
                            LoginResult::Error(err) => {
                                view.state = GenericState::Error(err.clone());
                                cx.emit(login_result);
                            }
                            LoginResult::BackToDiscovery => {
                                view.state = GenericState::Default;
                                cx.emit(login_result);
                            }
                        },
                        Err(_cancelled) => {
                            view.state = GenericState::Default;
                        }
                    }
                    cx.notify();
                }) {
                    tracing::error!("Failed to update login state: {e:?}");
                };
            });
        })
        .detach();
    }

    fn on_back_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(LoginResult::BackToDiscovery);
    }

    fn render_login_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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

        let mut card = floating_tile(theme, structure)
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
            );

        if !self.login_types_loaded {
            return card.child(
                div()
                    .w_full()
                    .py(structure.gap * 2)
                    .text_center()
                    .text_sm()
                    .text_color(theme.text.dim)
                    .child("Checking login options..."),
            );
        }

        if self.sso_only {
            card = card.child(
                Button::new("login-sso")
                    .label("Log in via SSO")
                    .w_full()
                    .on_click(cx.listener(Self::on_sso_login_click)),
            );
        } else {
            card = card
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
                );

            if self.has_sso {
                card = card.child(
                    div()
                        .flex_col()
                        .flex()
                        .gap(structure.small_gap)
                        .child("or")
                        .text_color(theme.text.dim)
                        .child(
                            Button::new("login-sso")
                                .label("Use SSO")
                                .w_full()
                                .on_click(cx.listener(Self::on_sso_login_click)),
                        ),
                );
            }
        }

        card
    }
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(self.render_login_form(window, cx))
    }
}
