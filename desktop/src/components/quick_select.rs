use deplace_core::{
    matrix_api::account_data::BreadcrumbsContent,
    state::{AppState, MembershipMap},
};
use gpui::*;
use gpui_component::input::InputState;
use matrix_sdk::ruma::OwnedUserId;
use tokio::sync::watch::Receiver;

use crate::{
    components::{cache::AvatarCache, floating_tile, overlay::Close},
    theme::DeplaceThings,
};

actions!(quick_select, [Open]);

pub struct QuickSelect {
    focus: FocusHandle,

    search_state: Entity<InputState>,
    breadcrumbs: BreadcrumbsContent,

    membership_map: Receiver<MembershipMap>,
    own_id: OwnedUserId,
    avatar_cache: AvatarCache,
}

impl QuickSelect {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: AppState,
        avatar_cache: AvatarCache,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let search_state = cx.new(|cx| InputState::new(window, cx));

        let breadcrumbs = state.breadcrumbs();
        let membership_map = state.membership_map();
        let own_id = state.user_device().user_id.clone();
        let avatar_cache = avatar_cache.clone();

        // state.client.resolve_room_alias(room_alias);

        Self {
            focus,
            search_state,
            breadcrumbs,
            membership_map,
            own_id,
            avatar_cache,
        }
    }
}

impl EventEmitter<Close> for QuickSelect {}

impl Focusable for QuickSelect {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for QuickSelect {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::prelude::Context<Self>,
    ) -> impl gpui::prelude::IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        floating_tile(theme, structure)
            .track_focus(&self.focus)
            .id("quick-select")
            .key_context("QuickSelect")
            .on_click(|_event, _window, cx| {
                cx.stop_propagation();
            })
            .on_action(cx.listener(|_, _: &Close, _, cx| {
                tracing::debug!("Closing quick select");
                cx.emit(Close);
            }))
            .flex()
            .flex_col()
            .w_1_2()
            .max_w(px(600.0))
            .h(px(400.0))
            .overflow_hidden()
            .p_4()
    }
}
