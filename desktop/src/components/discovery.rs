use std::{str::FromStr, sync::Arc};

use deplace_core::matrix_api::test_server;
use gpui::{
    AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, Subscription,
    Window, div, prelude::*,
};
use gpui_component::{
    Disableable, StyledExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    input::{InputEvent, InputState},
};
use matrix_sdk::{Client, reqwest::Url};

use crate::{
    GenericState,
    components::{floating_tile, input},
    theme::ActiveAppTheme,
};

pub struct DiscoveryView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    pub state: GenericState,
    server_input: Entity<InputState>,
    valid_result: Option<(Client, Url)>,
    current_request: Option<tokio::task::AbortHandle>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Option<(Client, Url)>> for DiscoveryView {}

impl DiscoveryView {
    pub fn new(
        tokio_rt: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let server_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://matrix.example.org")
                .default_value("erik-is.gay")
        });

        let _subscriptions =
            vec![
                cx.subscribe_in(&server_input, window, |this, _, _: &InputEvent, _, cx| {
                    this.test_server(cx);
                }),
            ];

        let mut view = Self {
            tokio_rt,
            state: GenericState::default(),
            valid_result: None,
            current_request: None,
            server_input,
            _subscriptions,
        };
        view.test_server(cx);
        view
    }

    fn check_input(&mut self, cx: &mut Context<Self>) -> Option<Url> {
        let mut text = self.server_input.read(cx).value().to_string();

        if !text.starts_with("https://") {
            text = format!("https://{}", text);
        }

        if text.is_empty() {
            self.state = GenericState::Disabled;
            cx.notify();
            return None;
        }

        match Url::from_str(text.as_ref()) {
            Ok(url) => Some(url),
            Err(_) => {
                self.state = GenericState::Error("Enter a valid server URL".to_string());
                cx.notify();
                None
            }
        }
    }

    fn on_discover_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let event = self.valid_result.clone();

        cx.spawn(async move |this, cx| {
            cx.update(|cx| {
                let _ = this.update(cx, |_, cx| {
                    cx.emit(event);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn test_server(&mut self, cx: &mut Context<Self>) {
        let tokio_rt = Arc::clone(&self.tokio_rt);

        let Some(url) = self.check_input(cx) else {
            return;
        };

        if let Some(handle) = self.current_request.take() {
            handle.abort();
        }

        self.state = GenericState::Loading;
        cx.notify();

        let task = tokio_rt.spawn(async move { test_server(url.to_string()).await });
        self.current_request = Some(task.abort_handle());

        cx.spawn(async move |this, cx| {
            // If this task was aborted by a newer request, just drop the update.
            let Ok(result) = task.await else {
                return;
            };

            cx.update(|cx| {
                let _ = this.update(cx, |view, cx| {
                    if result.is_none() {
                        view.state = GenericState::Error("Couldn't reach that server".to_string());
                    } else {
                        view.state = GenericState::Success;
                    }
                    view.valid_result = result;
                    view.current_request = None;
                    cx.notify();
                });
            });
        })
        .detach();
    }
}

impl Render for DiscoveryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.state.is_disabled();

        let theme = cx.app_theme();

        let (message, color) = match &self.state {
            GenericState::Default => ("Ready to discover".to_string(), theme.colors.muted),
            GenericState::Loading | GenericState::Disabled => {
                ("Discovering...".to_string(), theme.colors.muted)
            }
            GenericState::Error(err) => (err.clone(), theme.colors.error),
            GenericState::Success => ("Discovered successfully".to_string(), theme.colors.success),
        };

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                floating_tile(theme)
                    .flex_col()
                    .w(gpui::px(320.))
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_2xl()
                            .font_extrabold()
                            .text_color(theme.accent)
                            .child("Discovery"),
                    )
                    .child(
                        div()
                            .flex_col()
                            .flex()
                            .gap(theme.small_gap)
                            .child("Homeserver")
                            .text_color(theme.text.dim)
                            .child(input(theme, &self.server_input, window, cx)),
                    )
                    .child(
                        div()
                            .flex_col()
                            .flex()
                            .gap(theme.small_gap)
                            .child(div().child(message).text_color(color))
                            .child({
                                let bg = if disabled {
                                    theme.colors.muted
                                } else {
                                    theme.accent
                                };
                                let variant =
                                    ButtonCustomVariant::new(cx).color(bg).hover(bg).active(bg);

                                Button::new("server-submit")
                                    .label("Continue")
                                    .custom(variant)
                                    .w_full()
                                    .disabled(disabled)
                                    .cursor_pointer()
                                    .on_click(cx.listener(Self::on_discover_click))
                            }),
                    ),
            )
    }
}
