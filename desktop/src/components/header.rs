use std::sync::Arc;

use deplace_core::state::AppState;
use gpui::{Context, ParentElement, Render, Styled, div, white};
use matrix_sdk::Room;
use tokio::{runtime::Runtime, sync::watch};

use crate::watch_bridge::notify_on_change;

pub struct HeaderView {
    tokio_rt: Arc<Runtime>,
    active_room: watch::Receiver<Option<Room>>,
}

impl HeaderView {
    pub fn new(state: &AppState, cx: &mut Context<Self>, tokio_rt: Arc<Runtime>) -> Self {
        let active_room = state.active_room();

        notify_on_change(active_room.clone(), cx);

        Self {
            tokio_rt,
            active_room,
        }
    }
}

impl Render for HeaderView {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        let room = self.active_room.borrow().clone();
        let Some(room) = room else {
            return div().text_color(white()).child("No room selected");
        };

        div().text_color(white()).child(
            room.cached_display_name()
                .map(|n| n.to_string())
                .unwrap_or_default(),
        )
    }
}
