use std::cmp::Reverse;
use std::sync::Arc;

use deplace_core::{
    NameExt, RoomMap,
    matrix_api::sync::ParentToChildren,
    state::{AppState, MembershipMap, PresenceMap},
};
use gpui::{
    Context, EventEmitter, InteractiveElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, div, prelude::FluentBuilder, transparent_black,
};
use gpui_component::StyledExt;
use matrix_sdk::{Room, ruma::OwnedUserId};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{AvatarCache, home::ActiveRoomChange, profiles::render_room_icon},
    theme::DeplaceThings,
    watch_bridge::notify_on_change,
};

pub struct SidebarView {
    active_server: watch::Receiver<Option<Room>>,
    active_room: watch::Receiver<Option<Room>>,
    parent_to_children: watch::Receiver<ParentToChildren>,
    dm_rooms: watch::Receiver<RoomMap>,
    membership_map: watch::Receiver<MembershipMap>,
    presence_map: watch::Receiver<PresenceMap>,
    own_id: OwnedUserId,
    cache: AvatarCache,
}

impl EventEmitter<ActiveRoomChange> for SidebarView {}

impl SidebarView {
    pub fn new(
        state: AppState,
        cx: &mut Context<Self>,
        _tokio_rt: Arc<Runtime>, // Removed from struct if unused
        cache: AvatarCache,
    ) -> Self {
        let active_server = state.active_server();
        let active_room = state.active_room();
        let parent_to_children = state.parent_to_children();
        let dm_rooms = state.dm_rooms();
        let membership_map = state.membership_map();
        let presence_map = state.presence_map();

        notify_on_change(active_server.clone(), cx);
        notify_on_change(active_room.clone(), cx);
        notify_on_change(parent_to_children.clone(), cx);
        notify_on_change(dm_rooms.clone(), cx);
        notify_on_change(membership_map.clone(), cx);
        notify_on_change(presence_map.clone(), cx);
        notify_on_change(cache.subscribe(), cx);

        Self {
            active_server,
            active_room,
            parent_to_children,
            dm_rooms,
            membership_map,
            presence_map,
            own_id: state.user_device().user_id.clone(),
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
        let structure = cx.structure();

        // Borrow watch guards directly without cloning large HashMaps
        let membership_map = self.membership_map.borrow();
        let presence_map = self.presence_map.borrow();
        let active_server = self.active_server.borrow();

        let in_dms = active_server.is_none();
        let name = active_server
            .as_ref()
            .map(|room| room.get_name())
            .unwrap_or_else(|| "Direct Messages".to_string());

        let heading_font_size = structure.font_size * 1.1;
        let heading_padding = (structure.header.height - heading_font_size) / 2.0;

        let items = if let Some(server) = active_server.as_ref() {
            let mut children: Vec<(Room, Option<String>)> = self
                .parent_to_children
                .borrow()
                .get(server.room_id())
                .cloned()
                .unwrap_or_default()
                .values()
                .cloned()
                .collect();

            // Zero-allocation sorting
            children.sort_by(|(r1, o1), (r2, o2)| {
                let k1 = o1.as_deref().unwrap_or_else(|| r1.room_id().as_str());
                let k2 = o2.as_deref().unwrap_or_else(|| r2.room_id().as_str());
                k1.cmp(k2)
            });

            children.into_iter().map(|(room, _)| room).collect()
        } else {
            let mut dms: Vec<Room> = self.dm_rooms.borrow().values().cloned().collect();

            // Sort newest messages first
            dms.sort_by_key(|r| Reverse(r.latest_event_timestamp()));
            dms
        };

        let dm_icon_size = structure.sidebar.dm_icon_height;
        let channel_icon_size = structure.sidebar.channel_icon_height;

        let active_id = self
            .active_room
            .borrow()
            .as_ref()
            .map(|r| r.room_id().to_owned());

        let heights = move |in_dms| {
            let icon_height = if in_dms {
                dm_icon_size
            } else {
                channel_icon_size
            };
            let height = icon_height + structure.small_gap * 1.5;
            (icon_height, height)
        };

        let divs = items.into_iter().map(|room| {
            let room_name = room.get_name();
            let room_id = room.room_id().to_owned();
            let is_active = Some(&room_id) == active_id.as_ref();
            let (icon_height, height) = heights(in_dms);

            div()
                .border_1()
                .border_color(transparent_black())
                .px(structure.small_gap)
                .text_color(theme.text.dim)
                .items_center()
                .h(height)
                .flex()
                .flex_row()
                .gap(structure.gap)
                .rounded(structure.inner_border_radius)
                .hover(|style| {
                    style
                        .border_color(theme.tile.border)
                        .text_color(theme.text.normal)
                })
                .cursor_pointer()
                .when(is_active, |el| {
                    el.bg(theme.solid_hover_bg)
                        .text_color(theme.text.normal)
                        .border_color(theme.tile.border)
                        .cursor_default()
                })
                .id(room_id.to_string())
                .child(render_room_icon(
                    &room,
                    &membership_map,     // Borrowed reference passed directly
                    Some(&presence_map), // Borrowed reference passed directly
                    &self.own_id,
                    &self.cache,
                    icon_height,
                    icon_height / 2.0,
                    theme,
                ))
                .on_click(cx.listener({
                    move |_, _, _, cx| {
                        tracing::trace!("Room {} clicked", room_id);
                        cx.emit(ActiveRoomChange::new(Some(room.clone())));
                    }
                }))
                .child(room_name)
        });

        div()
            .w_full()
            .child(
                div()
                    .h(structure.header.height)
                    .flex()
                    .child(name)
                    .items_center()
                    .pl(heading_padding)
                    .font_bold()
                    .text_size(heading_font_size)
                    .border_b_1()
                    .border_color(theme.tile.border)
                    .text_color(theme.text.normal),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(structure.divider_width)
                    .px(structure.small_gap)
                    .py(structure.small_gap)
                    .children(divs),
            )
    }
}
