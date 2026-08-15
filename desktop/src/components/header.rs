use std::sync::Arc;

use deplace_core::{
    NameExt, get_other_member,
    state::{AppState, MembershipMap, PresenceMap},
};
use gpui::{Context, Element, ParentElement, Render, Styled, div, px};
use gpui_component::StyledExt;
use matrix_sdk::{Room, ruma::OwnedUserId};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{
        AvatarCache,
        profiles::{MemberRenderer, render_icon, render_room_icon},
    },
    theme::DeplaceThings,
    watch_bridge::notify_on_change,
};

pub struct HeaderView {
    tokio_rt: Arc<Runtime>,
    active_room: watch::Receiver<Option<Room>>,
    membership_map: watch::Receiver<MembershipMap>,
    presence_map: watch::Receiver<PresenceMap>,
    cache: AvatarCache,
    own_id: OwnedUserId,
}

impl HeaderView {
    pub fn new(
        state: AppState,
        cx: &mut Context<Self>,
        tokio_rt: Arc<Runtime>,
        cache: AvatarCache,
    ) -> Self {
        let active_room = state.active_room();
        let membership_map = state.membership_map();
        let presence_map = state.presence_map();

        notify_on_change(active_room.clone(), cx);
        notify_on_change(membership_map.clone(), cx);
        notify_on_change(presence_map.clone(), cx);

        Self {
            tokio_rt,
            active_room,
            membership_map,
            presence_map,
            cache,
            own_id: state.user_device().user_id.clone(),
        }
    }
}

impl Render for HeaderView {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        let room = self.active_room.borrow().clone();

        let theme = cx.app_theme();
        let structure = cx.structure();

        let icon_size = structure.header.icon_size;

        let name = room
            .as_ref()
            .map(|r| r.get_name())
            .unwrap_or("No room selected".to_string());

        let membership_map = self.membership_map.borrow().clone();
        let presence_map = self.presence_map.borrow().clone();

        let name_div = div().child(name).into_any();

        div()
            .size_full()
            .text_color(theme.text.normal)
            .paddings(structure.gap)
            .flex()
            .items_center()
            .flex_row()
            .gap(structure.gap)
            .child(
                div()
                    .child(if let Some(room) = &room {
                        render_room_icon(
                            room,
                            &membership_map,
                            &presence_map,
                            &self.own_id,
                            &self.cache,
                            icon_size,
                            icon_size / 2.0,
                            theme,
                        )
                    } else {
                        render_icon(phosphor_svgs::icon::aperture::BOLD, icon_size)
                    })
                    .pl(structure.header.icon_padding() - structure.gap)
                    .text_color(theme.text.dim),
            )
            .child(
                if let Some(room) = room
                    && room.is_dm()
                {
                    get_other_member(&self.own_id, &membership_map, room.room_id())
                        .map(|m| m.render_name(px(16.0), &theme.colors))
                        .unwrap_or(name_div)
                } else {
                    name_div
                },
            )
    }
}
