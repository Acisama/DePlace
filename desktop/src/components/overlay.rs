use gpui::*;

use crate::components::quick_select::QuickSelect;

actions!(overlay, [Close]);

#[derive(Debug)]
pub struct Overlay {
    focus: FocusHandle,
    content: OverlayContent,
    // persist the close subscription in order to be
    // able to close it later on
    _subscription: Option<Subscription>,
}

#[derive(Debug)]
pub enum OverlayContent {
    None,
    Settings,
    QuickSelect(Entity<QuickSelect>),
}

/// All interaction should happen with these functions to make everything more clean
impl Overlay {
    /// Initialize the Overlay, designed to be called in `cx.new(...)` in order to instantiate
    /// a new `Entity<Overlay>` for the base struct
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        Self {
            focus,
            content: OverlayContent::None,
            _subscription: None,
        }
    }

    /// Open quickselect and focus the overlay and the quickselect menu
    pub fn open_quick_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let quick_select = cx.new(|cx| QuickSelect::new(window, cx));

        // Subscribe to Close events emitted by QuickSelect
        cx.subscribe(&quick_select, |_this, _, _event: &Close, cx| {
            cx.emit(Close); // Re-emit Close from Overlay so home catches it
        })
        .detach();

        window.focus(&self.focus, cx);
        window.focus(&quick_select.focus_handle(cx), cx);

        self.content = OverlayContent::QuickSelect(quick_select);
    }

    /// Open the Settings and focus self for now
    ///
    /// TODO: Add settings so that they can be displayed and focused
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);

        self.content = OverlayContent::Settings;
    }

    /// Close the overlay
    ///
    /// Isn't automatically called in order to let the parent decide whether
    /// closing should happen when the action is invoked
    pub fn close_overlay(&mut self, _window: &mut Window, _cx: &mut App) {
        self.content = OverlayContent::None;
    }
}

impl EventEmitter<Close> for Overlay {}

impl Render for Overlay {
    /// It is save to just always render the overlay, since it will return nothing if
    /// the content is `OverlayContent::None`
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if matches!(self.content, OverlayContent::None) {
            return Empty.into_any_element();
        }
        div()
            .id("overlay-backdrop")
            .key_context("Overlay")
            .track_focus(&self.focus)
            .size_full()
            .absolute()
            .top_0()
            .left_0()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|_this, _event, _window, cx| {
                cx.emit(Close);
            }))
            .child(match &self.content {
                OverlayContent::None => div().into_any_element(),
                OverlayContent::Settings => div().into_any_element(),
                OverlayContent::QuickSelect(ent) => ent.clone().into_any_element(),
            })
            .into_any_element()
    }
}
