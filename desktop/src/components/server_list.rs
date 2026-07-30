use std::{collections::HashMap, sync::Arc};

use deplace_core::state::AppState;
use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div, img, white};
use gpui_component::gray;
use matrix_sdk::{
    Room,
    ruma::{OwnedMxcUri, OwnedRoomId, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{MediaCache, gpui_format_from},
    watch_bridge::notify_on_change,
};

pub struct ServerListView {
    cache: MediaCache<OwnedMxcUri>,
    rooms: watch::Receiver<HashMap<OwnedRoomId, Room>>,
}

impl ServerListView {
    pub fn new(state: &AppState, cx: &mut Context<Self>, tokio_rt: Arc<Runtime>) -> Self {
        let rooms = state.server_rooms();
        let cache = MediaCache::new(state.client.clone(), tokio_rt);

        notify_on_change(rooms.clone(), cx);
        notify_on_change(cache.subscribe(), cx);

        Self { rooms, cache }
    }
}

impl Render for ServerListView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rooms: Vec<_> = self
            .rooms
            .borrow()
            .values()
            .map(|room| {
                let bytes = room.avatar_url().and_then(|url| {
                    let source = MediaSource::Plain(url);
                    self.cache.get(&source)
                });

                if let Some(bytes) = bytes {
                    let format = image::guess_format(&bytes).unwrap_or(image::ImageFormat::Png);
                    let image = Arc::new(gpui::Image::from_bytes(
                        gpui_format_from(format),
                        bytes.to_vec(),
                    ));
                    img(image).size_6().rounded_full().into_any_element()
                } else {
                    div().size_6().rounded_full().bg(white()).into_any_element()
                }
            })
            .collect();

        div().flex().flex_col().children(rooms)
    }
}
