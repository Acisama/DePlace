use std::{collections::HashMap, sync::Arc};

use deplace_core::{
    colors::Color,
    matrix_api::account_data::{ServerOrderContent, get_account_data, set_account_data},
    state::AppState,
};
use gpui::{
    Context, EventEmitter, IntoElement, ParentElement, Render, Styled, Window, div,
    transparent_black,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use matrix_sdk::{
    Room, RoomDisplayName,
    ruma::{OwnedMxcUri, OwnedRoomId, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{ActiveRoomChange, MediaCache, avatar, gpui_format_from},
    theme::ActiveAppTheme,
    watch_bridge::notify_on_change,
};

pub struct ServerListView {
    state: AppState,
    tokio_rt: Arc<Runtime>,
    cache: MediaCache<OwnedMxcUri>,
    rooms: watch::Receiver<HashMap<OwnedRoomId, Room>>,
    server_order: Vec<OwnedRoomId>,
}

impl EventEmitter<ActiveRoomChange> for ServerListView {}

impl ServerListView {
    pub fn new(state: &AppState, cx: &mut Context<Self>, tokio_rt: Arc<Runtime>) -> Self {
        let rooms = state.server_rooms();
        let cache = MediaCache::new(state.client.clone(), tokio_rt.clone());

        notify_on_change(rooms.clone(), cx);
        notify_on_change(cache.subscribe(), cx);

        let client = state.client.clone();
        let task =
            tokio_rt.spawn(async move { get_account_data::<ServerOrderContent>(&client).await });
        cx.spawn(async move |this, cx| {
            let Ok(order) = task.await else {
                return;
            };

            cx.update(|cx| {
                let _ = this.update(cx, |view, cx| {
                    view.server_order = order.servers;
                    cx.notify();
                });
            });
        })
        .detach();

        Self {
            state: state.clone(),
            tokio_rt,
            rooms,
            cache,
            server_order: Vec::new(),
        }
    }

    fn set_server_order(&mut self, servers: Vec<OwnedRoomId>) {
        self.server_order = servers.clone();

        let client = self.state.client.clone();
        self.tokio_rt.spawn(async move {
            set_account_data(&client, ServerOrderContent { servers }).await;
        });
    }
}

impl Render for ServerListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let icon_size = theme.structure.server_column.icon_width;
        let rounding = icon_size / 4.0;

        let rooms_map = self.rooms.borrow().clone();
        let mut sorted_rooms = rooms_map.values().cloned().collect::<Vec<_>>();

        for room_id in rooms_map.keys() {
            if let Some(index) = self.server_order.iter().position(|id| id == room_id) {
                sorted_rooms.swap(index, 0);
            }
        }

        let new_server_order: Vec<_> = sorted_rooms
            .iter()
            .map(|room| room.room_id().to_owned())
            .collect();
        if new_server_order != self.server_order {
            self.set_server_order(new_server_order);
        }

        let icons: Vec<_> = sorted_rooms
            .into_iter()
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
                let room_id = room.room_id().to_owned();

                let variant = ButtonCustomVariant::new(cx)
                    .color(transparent_black())
                    .active(transparent_black())
                    .foreground(transparent_black())
                    .hover(transparent_black())
                    .shadow(false);

                Button::new(format!("server-{}", room_id))
                    .on_click(cx.listener({
                        move |_, _, _, cx| {
                            tracing::trace!("Server {} clicked", room_id);
                            cx.emit(ActiveRoomChange::SetServer(room.clone()));
                        }
                    }))
                    .custom(variant)
                    .size(icon_size)
                    .p_0()
                    .child(avatar(initial, color, icon_size, rounding, image))
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
