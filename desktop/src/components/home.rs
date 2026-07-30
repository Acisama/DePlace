use std::sync::Arc;

use deplace_core::state::AppState;
use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div, green};

pub struct HomeView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    state: AppState,
}

impl HomeView {
    pub fn new(tokio_rt: Arc<tokio::runtime::Runtime>, state: AppState) -> Self {
        Self { tokio_rt, state }
    }
}

impl Render for HomeView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .bg(green())
            .size_full()
            .justify_center()
            .items_center()
            .child("Home...")
    }
}
