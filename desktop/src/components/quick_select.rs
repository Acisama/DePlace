use gpui::*;

use crate::{
    components::floating_tile,
    theme::{ActiveAppTheme, StructureExt},
};

actions!(quick_select, [Open, Close]);

pub struct QuickSelect {
    focus: FocusHandle,
}

impl QuickSelect {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        Self { focus }
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
            // // --- Elevation & Borders ---
            // .border_1()
            // .border_color(rgb(0x313244))
            // .shadow_lg()
            // // Internal spacing
            .p_4()
    }
}
