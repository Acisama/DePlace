use std::{collections::HashSet, sync::Arc};

use deplace_core::{
    NameExt,
    state::{AppState, MembershipMap},
};
use gpui::{prelude::FluentBuilder, *};
use gpui_component::input::{Input, InputState};
use matrix_sdk::{
    room::Room,
    ruma::{OwnedRoomId, OwnedUserId},
};
use nucleo::{
    Config, Nucleo,
    pattern::{CaseMatching, Normalization},
};
use serde::Deserialize;
use tokio::sync::watch::Receiver;

use crate::{
    cache::AvatarCache,
    components::{floating_tile, profiles::render_room_icon},
    theme::DeplaceThings,
};

actions!(quick_select, [FocusNext, FocusPrevious, Confirm]);

#[derive(Clone, Debug, PartialEq, Deserialize, gpui::Action, schemars::JsonSchema)]
#[action(namespace = quick_select)]
pub struct Close {
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub room_id: Option<OwnedRoomId>,
}

pub struct QuickSelect {
    focus: FocusHandle,
    scroll_handle: UniformListScrollHandle,

    state: AppState,

    search_state: Entity<InputState>,
    last_query: String,

    membership_map: Receiver<MembershipMap>,
    own_id: OwnedUserId,
    avatar_cache: AvatarCache,

    focus_entry: usize,

    matcher: Nucleo<OwnedRoomId>,
}

impl QuickSelect {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        state: AppState,
        avatar_cache: AvatarCache,
    ) -> Self {
        let focus = cx.focus_handle();

        let search_state = cx.new(|cx| InputState::new(window, cx));
        let last_query = search_state.read(cx).text().to_string();

        let membership_map = state.membership_map();
        let own_id = state.user_device().user_id.clone();
        let avatar_cache = avatar_cache.clone();

        let matcher = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), None, 1);

        let injector = matcher.injector();
        let client = state.client();

        let mut added_rooms = HashSet::new();
        let mut add_room = |room: Room| {
            let room_id = room.room_id().to_owned();
            if added_rooms.insert(room_id.clone()) {
                injector.push(room_id, |_, dst| {
                    dst[0] = room.get_name().into();
                });
            }
        };

        // TODO: Filter the rooms
        for room in client.rooms() {
            add_room(room);
        }
        for room in state.dm_rooms().borrow().values() {
            add_room(room.clone());
        }
        for room in state.server_rooms().borrow().values() {
            add_room(room.clone());
        }
        for room in state.single_rooms().borrow().values() {
            add_room(room.clone());
        }

        // Observe search input changes only when the text content actually changes
        cx.observe(&search_state, |this, search_state, cx| {
            let text = search_state.read(cx).text().to_string();
            if text != this.last_query {
                this.last_query = text.clone();
                this.matcher.pattern.reparse(
                    0,
                    &text,
                    CaseMatching::Ignore,
                    Normalization::Smart,
                    false,
                );
                this.focus_entry = 0;
                this.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
        })
        .detach();

        Self {
            focus,
            scroll_handle: UniformListScrollHandle::new(),
            state,
            search_state,
            last_query,
            membership_map,
            own_id,
            avatar_cache,
            focus_entry: 0,
            matcher,
        }
    }

    fn get_displayed_rooms(&self, cx: &App) -> Vec<OwnedRoomId> {
        let text = self.search_state.read(cx).text().to_string();
        if text.trim().is_empty() {
            let client = self.state.client();
            self.state
                .breadcrumbs()
                .recent_rooms()
                .iter()
                .skip(1)
                .filter(|id| client.get_room(id).is_some())
                .cloned()
                .collect()
        } else {
            let snapshot = self.matcher.snapshot();
            let matched_count = snapshot.matched_item_count();
            snapshot
                .matched_items(0..matched_count)
                .map(|item| item.data.clone())
                .collect()
        }
    }
}

impl EventEmitter<Close> for QuickSelect {}

impl Focusable for QuickSelect {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search_state.read(cx).focus_handle(cx)
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

        self.matcher.tick(10);
        let matched_rooms = self.get_displayed_rooms(cx);

        let client = self.state.client().clone();
        let membership_map = self.membership_map.clone();
        let own_id = self.own_id.clone();
        let avatar_cache = self.avatar_cache.clone();
        let focus_entry = self.focus_entry;

        floating_tile(theme, structure)
            .track_focus(&self.focus)
            .id("quick-select")
            .key_context("QuickSelect")
            .on_click(|_event, _window, cx| {
                cx.stop_propagation();
            })
            .on_action(cx.listener(|_, action: &Close, _, cx| {
                tracing::debug!("Closing quick select with room: {:?}", action.room_id);
                cx.emit(action.clone());
            }))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                let displayed_rooms = this.get_displayed_rooms(cx);
                let selected_room = displayed_rooms.get(this.focus_entry).cloned();

                tracing::debug!("Confirming selection: {:?}", selected_room);
                window.dispatch_action(
                    Box::new(Close {
                        room_id: selected_room,
                    }),
                    cx,
                );
            }))
            .on_action(cx.listener(|this, _: &FocusNext, _, cx| {
                tracing::debug!("Focusing next quick select entry");
                let count = this.get_displayed_rooms(cx).len();
                if count > 0 && this.focus_entry + 1 < count {
                    this.focus_entry += 1;
                    this.scroll_handle
                        .scroll_to_item(this.focus_entry, ScrollStrategy::Bottom);
                    tracing::debug!("Focusing index {}", this.focus_entry);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &FocusPrevious, _, cx| {
                tracing::debug!("Focusing previous quick select entry");
                if this.focus_entry > 0 {
                    this.focus_entry -= 1;
                    this.scroll_handle
                        .scroll_to_item(this.focus_entry, ScrollStrategy::Top);
                    tracing::debug!("Focusing index {}", this.focus_entry);
                    cx.notify();
                }
            }))
            .flex()
            .flex_col()
            .w_1_2()
            .max_w(px(600.0))
            .h(px(400.0))
            .overflow_hidden()
            .p_4()
            .gap_3()
            .child(
                div()
                    .id("quick-select-input-container")
                    .w_full()
                    .flex()
                    .items_center()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .border_1()
                    .bg(theme.input.background)
                    .focus(|style| style.border_color(theme.accent))
                    .text_color(theme.text.normal)
                    .child(Input::new(&self.search_state)),
            )
            .child(
                uniform_list(
                    "quick-select-list",
                    matched_rooms.len(),
                    move |range, _window, cx| {
                        let theme = cx.app_theme();

                        range
                            .map(|id| {
                                let room_id = &matched_rooms[id];
                                let Some(room) = client.get_room(room_id) else {
                                    return Empty.into_any_element();
                                };

                                div()
                                    .id(SharedString::from(format!("item-{room_id}")))
                                    .w_full()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2p5()
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .border_1()
                                    .border_color(gpui::transparent_white())
                                    .when(id == focus_entry, |el| {
                                        el.border_color(theme.accent).bg(theme.solid_hover_bg)
                                    })
                                    .hover(|style| style.bg(theme.solid_hover_bg))
                                    .on_click({
                                        let room_id = room_id.clone();
                                        move |_event, window, cx| {
                                            window.dispatch_action(
                                                Box::new(Close {
                                                    room_id: Some(room_id.clone()),
                                                }),
                                                cx,
                                            );
                                        }
                                    })
                                    .child(render_room_icon(
                                        &room,
                                        &membership_map.borrow(),
                                        None,
                                        &own_id,
                                        &avatar_cache,
                                        Pixels::from(30.0),
                                        Pixels::from(15.0),
                                        theme,
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_color(theme.text.normal)
                                            .child(room.get_name()),
                                    )
                                    .into_any_element()
                            })
                            .collect()
                    },
                )
                .track_scroll(&self.scroll_handle)
                .flex_1()
                .w_full(),
            )
    }
}
