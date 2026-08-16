use std::sync::Arc;

use deplace_core::settings::Settings;
use gpui::{
    App, Context, Element, EventEmitter, FocusHandle, Focusable, InteractiveElement, Render,
    StatefulInteractiveElement, UniformListScrollHandle, actions, div,
};
use tokio::runtime::Runtime;

actions!(settings, [Close]);

pub struct SettingsView {
    settings: Settings,
    tokio_rt: Arc<Runtime>,

    focus: FocusHandle,
    scroll_handle: UniformListScrollHandle,
}

impl SettingsView {
    pub fn new(cx: &mut Context<Self>, settings: Settings, tokio_rt: Arc<Runtime>) -> Self {
        let focus = cx.focus_handle();

        Self {
            settings,
            tokio_rt,
            scroll_handle: UniformListScrollHandle::new(),
            focus,
        }
    }
}

impl EventEmitter<Close> for SettingsView {}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.focus)
            .on_click(|_event, _window, cx| {
                cx.stop_propagation();
            })
            .on_action(cx.listener(|_, _: &Close, _, cx| cx.emit(Close)))
            .into_any()
    }
}
