use std::sync::Arc;

use deplace_core::{
    RoomMap, get_dm_room_name, get_room_name,
    matrix_api::sync::ParentToChildren,
    state::{AppState, MembershipMap},
};
use gpui::{
    AnyElement, Context, Div, Element, IntoElement, ParentElement, Render, Styled, div, px,
};
use gpui_component::StyledExt;
use matrix_sdk::{Room, ruma::OwnedUserId};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{AvatarCache, render_room_avatar, render_room_icon},
    theme::ActiveAppTheme,
    watch_bridge::notify_on_change,
};

pub struct SidebarView {
    active_server: watch::Receiver<Option<Room>>,
    parent_to_children: watch::Receiver<ParentToChildren>,
    dm_rooms: watch::Receiver<RoomMap>,
    tokio_rt: Arc<Runtime>,
    membership_map: watch::Receiver<MembershipMap>,
    own_id: OwnedUserId,
    cache: AvatarCache,
}

impl SidebarView {
    pub fn new(
        state: &AppState,
        cx: &mut Context<Self>,
        tokio_rt: Arc<Runtime>,
        cache: AvatarCache,
    ) -> Self {
        let active_server = state.active_server();
        let parent_to_children = state.parent_to_children();
        let dm_rooms = state.dm_rooms();
        let membership_map = state.membership_map();

        notify_on_change(active_server.clone(), cx);
        notify_on_change(parent_to_children.clone(), cx);
        notify_on_change(dm_rooms.clone(), cx);
        notify_on_change(membership_map.clone(), cx);

        Self {
            active_server,
            parent_to_children,
            dm_rooms,
            tokio_rt,
            membership_map,
            own_id: state.user_device.user_id.clone(),
            cache,
        }
    }
}

impl Render for SidebarView {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        let theme = cx.app_theme();

        let membership_map = self.membership_map.borrow().clone();
        let own_id = self.own_id.clone();
        let cache = self.cache.clone();

        let active_server = self.active_server.borrow().clone();
        let in_dms = active_server.is_none();
        let name = active_server
            .as_ref()
            .map(get_room_name)
            .unwrap_or("Direct Messages".to_string());

        let heading_font_size = theme.text.font_size * 1.1;
        let heading_padding = (theme.structure.header_height - heading_font_size) / 2.0;

        let items = if let Some(server) = active_server {
            self.parent_to_children
                .borrow()
                .get(server.room_id())
                .cloned()
                .unwrap_or_default()
        } else {
            self.dm_rooms.borrow().values().cloned().collect::<Vec<_>>()
        };

        let divs = items.into_iter().map(|item| {
            if in_dms {
                render_dm_room(&item, &membership_map, &own_id, &cache)
            } else {
                render_server_room(&item)
            }
        });

        div()
            .w_full()
            .child(
                div()
                    .h(theme.structure.header_height)
                    .flex()
                    .child(name)
                    .items_center()
                    .pl(heading_padding)
                    .font_bold()
                    .text_size(theme.text.font_size * 1.1)
                    .border_b_1()
                    .border_color(theme.tile.border)
                    .text_color(theme.text.normal),
            )
            .children(divs)
    }
}

fn render_dm_room(
    room: &Room,
    map: &MembershipMap,
    own_id: &OwnedUserId,
    cache: &AvatarCache,
) -> AnyElement {
    let name = get_dm_room_name(room, map, own_id);
    div()
        .child(render_room_avatar(
            room,
            map,
            own_id,
            cache,
            px(20.0),
            px(4.0),
        ))
        .into_any()
}

fn render_server_room(room: &Room) -> AnyElement {
    let name = get_room_name(room);
    div().child(render_room_icon(room, px(20.0))).into_any()
}
