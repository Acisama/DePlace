use std::sync::Arc;

use deplace_core::state::AppState;
use gpui::*;
use matrix_sdk::ruma::OwnedRoomId;
use serde::Deserialize;
use tokio::runtime::Runtime;

use crate::{
    cache::AvatarCache,
    components::{
        quick_select::{self, QuickSelect},
        settings::{self, SettingsView},
    },
    things::DeplaceThings,
};

#[derive(Clone, Debug, PartialEq, Deserialize, gpui::Action, schemars::JsonSchema)]
#[action(namespace = overlay)]
pub enum Close {
    Settings,
    #[schemars(with = "Option<String>")]
    QuickSelect(Option<OwnedRoomId>),
}

#[derive(Clone, Debug, PartialEq, Deserialize, gpui::Action, schemars::JsonSchema)]
#[action(namespace = overlay)]
pub enum Open {
    Settings,
    QuickSelect,
}

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
    Settings(Entity<SettingsView>),
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
    pub fn open_quick_select(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        state: AppState,
        avatar_cache: AvatarCache,
    ) {
        let quick_select = cx.new(|cx| QuickSelect::new(window, cx, state, avatar_cache));

        // Subscribe to Close events emitted by QuickSelect
        cx.subscribe(
            &quick_select,
            |_this, _, event: &quick_select::Close, cx| {
                cx.emit(Close::QuickSelect(event.room_id.clone())); // Re-emit Close from Overlay so home catches it
            },
        )
        .detach();

        window.focus(&self.focus, cx);
        window.focus(&quick_select.focus_handle(cx), cx);

        self.content = OverlayContent::QuickSelect(quick_select);
    }

    /// Open the Settings and focus self for now
    pub fn open_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        state: AppState,
        tokio_rt: Arc<Runtime>,
    ) {
        let settings = cx.new(|cx| SettingsView::new(cx, state.settings(), tokio_rt));

        cx.subscribe(&settings, |_this, _, _: &settings::Close, cx| {
            cx.emit(Close::Settings); // Re-emit Close from Overlay so home catches it
        })
        .detach();

        window.focus(&self.focus, cx);
        window.focus(&settings.focus_handle(cx), cx);

        self.content = OverlayContent::Settings(settings);
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
        let structure = cx.structure();
        let settings_size = StyleRefinement::default()
            .w(structure.settings.full_width)
            .h(structure.settings.full_height);
        let quick_select_size = StyleRefinement::default()
            .w_1_2()
            .max_w(px(600.0))
            .h(px(400.0));

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
            .occlude()
            .on_click(cx.listener(|_this, _event, _window, cx| {
                cx.emit(Close::QuickSelect(None));
            }))
            .child(match &self.content {
                OverlayContent::None => div().into_any_element(),
                OverlayContent::Settings(ent) => {
                    ent.clone().cached(settings_size).into_any_element()
                }
                OverlayContent::QuickSelect(ent) => {
                    ent.clone().cached(quick_select_size).into_any_element()
                }
            })
            .into_any_element()
    }
}
