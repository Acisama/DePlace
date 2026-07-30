use std::{str::FromStr, sync::Arc};

use deplace_core::matrix_api::test_server;
use gpui::{
    AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, Subscription,
    Window, div, prelude::*, red,
};
use gpui_component::{
    Disableable,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    v_flex,
};
use matrix_sdk::{Client, reqwest::Url};

use crate::GenericState;

pub struct DiscoveryView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    pub state: GenericState,
    server_input: Entity<InputState>,
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

        let _subscriptions = vec![cx.subscribe_in(
            &server_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.test_server(cx);
                }
            },
        )];

        Self {
            tokio_rt,
            state: GenericState::default(),
            server_input,
            _subscriptions,
        }
    }

    fn check_input(&mut self, cx: &Context<Self>) -> Option<Url> {
        let mut text = self.server_input.read(cx).value().to_string();

        if !text.starts_with("https://") {
            text = format!("https://{}", text);
        }

        if text.is_empty() {
            self.state = GenericState::Default;
            return None;
        }
        Url::from_str(text.as_ref()).ok()
    }

    fn on_discover_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.test_server(cx);
    }

    fn test_server(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, GenericState::Loading) {
            return;
        }

        let tokio_rt = Arc::clone(&self.tokio_rt);

        let Some(url) = self.check_input(cx) else {
            self.state = GenericState::Error("Enter a valid server URL".to_string());
            cx.notify();
            return;
        };

        self.state = GenericState::Loading;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = tokio_rt
                .spawn(async move { test_server(url.to_string()).await })
                .await
                .unwrap();

            cx.update(|cx| {
                let _ = this.update(cx, |_view, cx| {
                    cx.emit(result);
                    cx.notify();
                });
            });
        })
        .detach();
    }
}

impl Render for DiscoveryView {
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
                    .child(Input::new(&self.server_input))
                    .when_some(error, |this, message| {
                        this.child(div().text_color(red()).child(message))
                    })
                    .child(
                        Button::new("discover-continue")
                            .label(if loading { "Searching…" } else { "Continue" })
                            .primary()
                            .w_full()
                            .loading(loading)
                            .disabled(loading)
                            .on_click(cx.listener(Self::on_discover_click)),
                    ),
            )
    }
}
