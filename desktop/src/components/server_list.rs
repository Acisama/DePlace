use std::{collections::HashMap, sync::Arc};

use deplace_core::{colors::Color, matrix_api::account_data::ServerOrderContent, state::AppState};
use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
use matrix_sdk::{
    Room, RoomDisplayName,
    ruma::{OwnedMxcUri, OwnedRoomId, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{MediaCache, avatar, gpui_format_from},
    theme::ActiveAppTheme,
    watch_bridge::notify_on_change,
};

pub struct ServerListView {
    cache: MediaCache<OwnedMxcUri>,
    rooms: watch::Receiver<HashMap<OwnedRoomId, Room>>,
    server_order: watch::Receiver<ServerOrderContent>,
}

impl ServerListView {
    pub fn new(state: &AppState, cx: &mut Context<Self>, tokio_rt: Arc<Runtime>) -> Self {
        let rooms = state.server_rooms();
        let cache = MediaCache::new(state.client.clone(), tokio_rt);
        let server_order = state.server_order();

        notify_on_change(rooms.clone(), cx);
        notify_on_change(cache.subscribe(), cx);
        notify_on_change(server_order.clone(), cx);

        Self {
            rooms,
            cache,
            server_order,
        }
    }
}

impl Render for ServerListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let icon_size = theme.structure.server_column.icon_width;
        let rounding = icon_size / 4.0;

        let rooms_map = self.rooms.borrow();
        let mut sorted_rooms = rooms_map.values().collect::<Vec<_>>();

        let server_order = self.server_order.borrow();
        for room_id in rooms_map.keys() {
            if let Some(index) = server_order.servers.iter().position(|id| id == room_id) {
                sorted_rooms.swap(index, 0);
            }
        }

        let icons: Vec<_> = sorted_rooms
            .iter()
            .map(|room| {
                let image = room.avatar_url().and_then(|url| {
                    let source = MediaSource::Plain(url);
                    let bytes = self.cache.get(&source)?;
                    let format = image::guess_format(&bytes).ok()?;
                    let image = Arc::new(gpui::Image::from_bytes(
                        gpui_format_from(format),
                        bytes.to_vec(),
                    ));
                    Some(image)
                });

                let initial = room
                    .cached_display_name()
                    .unwrap_or(RoomDisplayName::Empty)
                    .to_string()
                    .chars()
                    .next()
                    .unwrap_or('?');

                let color = Color::from(room.room_id().as_ref());

                avatar(initial, color, icon_size, rounding, image)
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .w_full()
            .items_center()
            .content_center()
            .gap(theme.tile.gap)
            .children(icons)
    }
}
