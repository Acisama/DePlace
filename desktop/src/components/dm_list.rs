use std::collections::HashMap;

use deplace_core::state::AppState;
use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
use matrix_sdk::{Room, ruma::OwnedRoomId};
use tokio::sync::watch;

use crate::watch_bridge::notify_on_change;

pub struct DmListView {
    rooms: watch::Receiver<HashMap<OwnedRoomId, Room>>,
}

impl DmListView {
    pub fn new(state: &AppState, cx: &mut Context<Self>) -> Self {
        let rooms = state.dm_rooms();
        notify_on_change(rooms.clone(), cx);
        Self { rooms }
    }
}

impl Render for DmListView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let room_ids: Vec<String> = self.rooms.borrow().keys().map(|id| id.to_string()).collect();

        div()
            .flex()
            .flex_col()
            .children(room_ids.into_iter().map(|id| div().child(id)))
    }
}
