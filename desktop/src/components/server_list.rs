use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use deplace_core::{
    matrix_api::account_data::{ServerOrderContent, set_account_data},
    state::AppState,
};
use gpui::{
    Context, Div, EventEmitter, IntoElement, ObjectFit, ParentElement, Pixels, Render, Styled,
    StyledImage, Window, div, img, prelude::FluentBuilder, transparent_black, white,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use matrix_sdk::{Room, ruma::OwnedRoomId};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    cache::AvatarCache,
    components::{home::ActiveServerChange, profiles::render_room_no_dm},
    theme::{AppTheme, DeplaceThings, Structure},
    watch_bridge::notify_on_change,
};

pub struct ServerListView {
    state: AppState,
    tokio_rt: Arc<Runtime>,
    cache: AvatarCache,
    servers: watch::Receiver<HashMap<OwnedRoomId, Room>>,
    server_order: watch::Receiver<Vec<OwnedRoomId>>,
    active_server: watch::Receiver<Option<Room>>,
    hovered_server: Option<Option<OwnedRoomId>>,
}

impl EventEmitter<ActiveServerChange> for ServerListView {}

impl ServerListView {
    pub fn new(
        state: AppState,
        cx: &mut Context<Self>,
        tokio_rt: Arc<Runtime>,
        cache: AvatarCache,
    ) -> Self {
        let rooms = state.server_rooms();
        let active_server = state.active_server().clone();

        notify_on_change(rooms.clone(), cx);
        notify_on_change(cache.subscribe(), cx);
        notify_on_change(active_server.clone(), cx);

        Self {
            state: state.clone(),
            tokio_rt,
            servers: rooms,
            cache,
            active_server,
            hovered_server: None,
            server_order: state.server_order().clone(),
        }
    }

    fn set_server_order(&mut self, server_order: Vec<OwnedRoomId>) {
        if server_order.is_empty() {
            return;
        }

        self.state.set_server_order(server_order.clone());

        let client = self.state.client();
        self.tokio_rt.spawn(async move {
            set_account_data(
                &client,
                ServerOrderContent {
                    servers: server_order,
                },
            )
            .await;
        });
    }
}

impl Render for ServerListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        let icon_size = structure.server_column.icon_width;
        let rounding = icon_size / 4.0;

        let pill_width = structure.small_gap / 2.0;

        let ordered_server_ids_vec = self.server_order.clone();

        let ordered_server_ids: HashSet<OwnedRoomId> =
            ordered_server_ids_vec.borrow().iter().cloned().collect();
        let all_server_ids: HashSet<OwnedRoomId> = self.servers.borrow().keys().cloned().collect();

        let unsorted_server_ids: Vec<OwnedRoomId> = all_server_ids
            .difference(&ordered_server_ids)
            .cloned()
            .collect();

        let active_server_id = self
            .active_server
            .borrow()
            .as_ref()
            .map(|s| s.room_id().to_owned());

        let rooms_map = self.servers.borrow().clone();
        let mut sorted_rooms: Vec<Room> = self
            .server_order
            .borrow()
            .iter()
            .filter_map(|id| rooms_map.get(id).cloned())
            .collect();

        let mut unsorted_rooms: Vec<Room> = unsorted_server_ids
            .iter()
            .filter_map(|id| rooms_map.get(id).cloned())
            .collect();
        unsorted_rooms.sort_by_key(|r| r.room_id().to_string());
        sorted_rooms.extend(unsorted_rooms);

        let new_server_order: Vec<_> = sorted_rooms
            .iter()
            .map(|room| room.room_id().to_owned())
            .collect();
        if new_server_order != *ordered_server_ids_vec.borrow() {
            self.set_server_order(new_server_order);
        }

        let variant = ButtonCustomVariant::new(cx)
            .color(transparent_black())
            .active(transparent_black())
            .foreground(transparent_black())
            .hover(transparent_black())
            .shadow(false);

        let icons = sorted_rooms.into_iter().map(|room| {
            let room_id = room.room_id().to_owned();

            let hovered = self
                .hovered_server
                .as_ref()
                .map_or_else(|| false, |o| o.as_ref() == Some(&room_id));

            pill(
                hovered,
                Some(&room_id) == active_server_id.as_ref(),
                false,
                pill_width,
                theme,
                structure,
            )
            .child(
                Button::new(format!("server-{}", room.room_id()))
                    .on_click(cx.listener({
                        let room_id = room_id.clone();
                        let room = room.clone();
                        move |_, _, _, cx| {
                            tracing::trace!("Server {} clicked", room_id);
                            cx.emit(ActiveServerChange::new(Some(room.clone())));
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
                    .child(render_room_no_dm(
                        &room,
                        &self.cache,
                        icon_size,
                        rounding,
                        false,
                    ))
                    .when(Some(room_id) == active_server_id, |el| {
                        el.child(
                            div()
                                .bg(white())
                                .absolute()
                                .inset_0()
                                .left(-2.5 * pill_width)
                                .h_full()
                                .rounded(pill_width / 2.0)
                                .w(pill_width),
                        )
                    }),
            )
        });

        div()
            .flex()
            .flex_col()
            .w_full()
            .items_center()
            .pt(2.0 * structure.small_gap)
            .content_center()
            .gap(structure.gap)
            .child(
                pill(
                    self.hovered_server.clone() == Some(None),
                    active_server_id.is_none(),
                    false,
                    pill_width,
                    theme,
                    structure,
                )
                .child(
                    Button::new("home-icon")
                        .on_click(cx.listener({
                            move |_, _, _, cx| {
                                tracing::trace!("Home icon clicked");
                                cx.emit(ActiveServerChange::new(None));
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
                    .h(structure.divider_width)
                    .w(icon_size)
                    .border_color(transparent_black())
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
    structure: &Structure,
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

    let icon_size = structure.server_column.icon_width;
    let rounding = pill_width / 2.0;
    let height = icon_size * scale_factor;

    div().size(icon_size).relative().child(
        div()
            .h(height)
            .w(pill_width)
            .rounded(rounding)
            .absolute()
            .left(-2.5 * pill_width)
            .top((icon_size - height) / 2.0)
            .bg(theme.pill_color),
    )
}
