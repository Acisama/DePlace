use std::sync::Arc;

use deplace_core::{RestoreResult, matrix_api::LoginResult, try_restore};
use gpui::{
    AppContext, Context, Entity, IntoElement, ObjectFit, ParentElement, Render, RenderImage,
    Styled, StyledImage, Window, blue, div, img,
};
use matrix_sdk::Client;

use crate::{
    GenericState,
    assets::decode_embedded_image,
    components::{discovery::DiscoveryView, home::HomeView, login::LoginView},
};

pub struct RootView {
    active_screen: Screen,
    tokio_rt: Arc<tokio::runtime::Runtime>,
    bg_image: Arc<RenderImage>,
}

#[derive(Default)]
enum Screen {
    #[default]
    Loading,
    ServerDiscovery(Entity<DiscoveryView>),
    Login(Entity<LoginView>),
    Home(Entity<HomeView>),
}

impl RootView {
    pub fn new(
        tokio_rt: Arc<tokio::runtime::Runtime>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let discovery_view = cx.new(|cx| DiscoveryView::new(Arc::clone(&tokio_rt), window, cx));

        let discovery_view_clone = discovery_view.clone();
        cx.subscribe_in(
            &discovery_view,
            window,
            move |this: &mut RootView, _child, event, window, cx| {
                if let Some((client, _url)) = event {
                    this.show_login(client.clone(), window, cx, discovery_view_clone.clone());
                }
            },
        )
        .detach();

        let restore_tokio_rt = Arc::clone(&tokio_rt);
        cx.spawn_in(window, async move |this, cx| {
            let outcome = restore_tokio_rt
                .spawn(async move { try_restore().await })
                .await;

            let _ = this.update_in(cx, |root, window, cx| {
                match outcome {
                    Ok(RestoreResult::Success(state)) => {
                        let tokio_rt = Arc::clone(&root.tokio_rt);
                        let home_view = cx.new(|cx| HomeView::new(tokio_rt, state, window, cx));
                        root.active_screen = Screen::Home(home_view);
                        // root.show_login(state.client, window, cx, discovery_view.clone());
                    }
                    Ok(RestoreResult::NoSession) => {
                        root.active_screen = Screen::ServerDiscovery(discovery_view.clone());
                    }
                    Ok(RestoreResult::NeedsLogin(client)) => {
                        root.show_login(client, window, cx, discovery_view.clone());
                    }
                    Err(_join_error) => {
                        discovery_view.update(cx, |view, _cx| {
                            view.state = GenericState::default();
                        });
                        root.active_screen = Screen::ServerDiscovery(discovery_view.clone());
                    }
                }
                cx.notify();
            });
        })
        .detach();

        Self {
            active_screen: Screen::default(),
            tokio_rt,
            bg_image: decode_embedded_image("bg.png").expect("failed to decode bg.png"),
        }
    }

    fn show_login(
        &mut self,
        client: Client,
        window: &mut Window,
        cx: &mut Context<Self>,
        discovery_view: Entity<DiscoveryView>,
    ) {
        let tokio_rt = Arc::clone(&self.tokio_rt);
        let login_view = cx.new(|cx| LoginView::new(tokio_rt, window, cx, client));

        cx.subscribe_in(&login_view, window, {
            let login_view = login_view.clone();
            move |this: &mut RootView, _child, event, window, cx| match event {
                LoginResult::Success(state) => {
                    let tokio_rt = Arc::clone(&this.tokio_rt);
                    let home_view = cx.new(|cx| HomeView::new(tokio_rt, state.clone(), window, cx));
                    this.active_screen = Screen::Home(home_view);
                    cx.notify();
                }
                LoginResult::InvalidCredentials => {
                    this.active_screen = Screen::Login(login_view.clone());
                    cx.notify();
                }
                LoginResult::Error(_error) => {
                    this.active_screen = Screen::Login(login_view.clone());
                    cx.notify();
                }
                LoginResult::BackToDiscovery => {
                    this.active_screen = Screen::ServerDiscovery(discovery_view.clone());
                    cx.notify();
                }
            }
        })
        .detach();

        self.active_screen = Screen::Login(login_view);
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .child(
                img(self.bg_image.clone())
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .absolute()
                    .inset_0(),
            )
            .child(match &self.active_screen {
                Screen::Loading => div().bg(blue()).child("Loading...").into_any_element(),
                Screen::ServerDiscovery(view) => view.clone().into_any_element(),
                Screen::Login(view) => view.clone().into_any_element(),
                Screen::Home(view) => view.clone().into_any_element(),
            })
    }
}
