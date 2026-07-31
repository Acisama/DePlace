use std::{collections::HashMap, sync::Arc};

use deplace_core::{
    colors::Color,
    matrix_api::account_data::{ServerOrderContent, get_account_data, set_account_data},
    state::AppState,
};
use gpui::{
    Context, Div, EventEmitter, IntoElement, ObjectFit, ParentElement, Pixels, Render, Styled,
    StyledImage, Window, div, img, prelude::FluentBuilder, transparent_black, white,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use matrix_sdk::{
    Room, RoomDisplayName,
    ruma::{OwnedMxcUri, OwnedRoomId, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{ActiveRoomChange, MediaCache, avatar, gpui_format_from},
    theme::{ActiveAppTheme, AppTheme},
    watch_bridge::notify_on_change,
};

pub struct ServerListView {
    state: AppState,
    tokio_rt: Arc<Runtime>,
    cache: MediaCache<OwnedMxcUri>,
    rooms: watch::Receiver<HashMap<OwnedRoomId, Room>>,
    server_order: Vec<OwnedRoomId>,
    active_server: watch::Receiver<Option<Room>>,
    hovered_server: Option<Option<OwnedRoomId>>,
}

impl EventEmitter<ActiveRoomChange> for ServerListView {}

impl ServerListView {
    pub fn new(state: &AppState, cx: &mut Context<Self>, tokio_rt: Arc<Runtime>) -> Self {
        let rooms = state.server_rooms();
        let cache = MediaCache::new(state.client.clone(), tokio_rt.clone());
        let active_server = state.active_server().clone();

        notify_on_change(rooms.clone(), cx);
        notify_on_change(cache.subscribe(), cx);
        notify_on_change(active_server.clone(), cx);

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
            active_server,
            hovered_server: None,
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
        let theme = cx.app_theme().clone();
        let icon_size = theme.structure.server_column.icon_width;
        let rounding = icon_size / 4.0;

        let pill_width = theme.gap / 2.5;

        let active_server_id = self
            .active_server
            .borrow()
            .as_ref()
            .map(|s| s.room_id().to_owned());

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

        let variant = ButtonCustomVariant::new(cx)
            .color(transparent_black())
            .active(transparent_black())
            .foreground(transparent_black())
            .hover(transparent_black())
            .shadow(false);

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

                let room_id = room.room_id().to_owned();
                let color = Color::from(room_id.as_ref());

                let hovered = self
                    .hovered_server
                    .as_ref()
                    .map_or_else(|| false, |o| o.as_ref() == Some(&room_id));

                pill(
                    hovered,
                    Some(&room_id) == active_server_id.as_ref(),
                    false,
                    pill_width,
                    &theme,
                )
                .child(
                    Button::new(format!("server-{}", room.room_id()))
                        .on_click(cx.listener({
                            let room_id = room_id.clone();
                            move |_, _, _, cx| {
                                tracing::trace!("Server {} clicked", room_id);
                                cx.emit(ActiveRoomChange::SetServer(Some(room.clone())));
                            }
                        }))
                        .on_hover(cx.listener({
                            let room_id = room_id.clone();
                            move |view, is_hovered: &bool, _, cx| {
                                view.hovered_server = is_hovered.then_some(Some(room_id.clone()));
                                cx.notify();
                            }
                        }))
                        .custom(variant)
                        .size(icon_size)
                        .p_0()
                        .relative()
                        .child(avatar(initial, color, icon_size, rounding, image))
                        .when(Some(room_id) == active_server_id, |el| {
                            el.child(
                                div()
                                    .bg(white())
                                    .absolute()
                                    .inset_0()
                                    .left(-2.0 * pill_width)
                                    .h_full()
                                    .rounded(pill_width / 2.0)
                                    .w(pill_width),
                            )
                        }),
                )
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .w_full()
            .items_center()
            .pt(1.5 * theme.gap)
            .content_center()
            .gap(theme.gap)
            .child(
                pill(
                    self.hovered_server.clone() == Some(None),
                    active_server_id.is_none(),
                    false,
                    pill_width,
                    &theme,
                )
                .child(
                    Button::new("home-icon")
                        .on_click(cx.listener({
                            move |_, _, _, cx| {
                                tracing::trace!("Home icon clicked");
                                cx.emit(ActiveRoomChange::SetServer(None));
                            }
                        }))
                        .on_hover(cx.listener(move |view, is_hovered: &bool, _, cx| {
                            view.hovered_server = is_hovered.then_some(None);
                            cx.notify();
                        }))
                        .custom(variant)
                        .p_0()
                        .cursor_pointer()
                        .size(icon_size)
                        .child(
                            div()
                                .size(icon_size)
                                .rounded(rounding)
                                .flex()
                                .items_center()
                                .justify_center()
                                .border_2()
                                .when_else(
                                    active_server_id.is_none(),
                                    |el| el.border_color(theme.accent),
                                    |el| el.border_color(transparent_black()),
                                )
                                .child(
                                    img("icon.png")
                                        .size(icon_size * 0.8)
                                        .object_fit(ObjectFit::Cover)
                                        .rounded(rounding),
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .h(theme.structure.divider_width)
                    .w(icon_size)
                    .border_color(transparent_black())
                    .my(theme.small_gap)
                    .when(active_server_id.is_none(), |el| {
                        el.border_color(theme.accent)
                    })
                    .bg(theme.tile.border),
            )
            .children(icons)
    }
}

fn pill(
    hovered: bool,
    active: bool,
    has_messages: bool,
    pill_width: Pixels,
    theme: &AppTheme,
) -> Div {
    let scale_factor = if active {
        1.0
    } else if hovered {
        0.5
    } else if has_messages {
        0.25
    } else {
        0.0
    };

    let icon_size = theme.structure.server_column.icon_width;
    let rounding = icon_size / 4.0;
    let height = icon_size * scale_factor;

    div().size(icon_size).relative().child(
        div()
            .h(height)
            .w(pill_width)
            .rounded(rounding)
            .absolute()
            .left(-2.0 * pill_width)
            .top((icon_size - height) / 2.0)
            .bg(theme.pill_color),
    )
}
