use deplace_core::{
    helpers::RoomPlaceholderExt,
    state::{AppState, MembershipMap},
};
use gpui::{
    AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, Styled, Window,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputEvent, InputState, RopeExt as _},
};
use macros::tailwind_div;
use matrix_sdk::{Room, ruma::OwnedRoomId};
use tokio::sync::watch::Receiver;

use crate::{
    components::{CustomStyles, chat::SendMessage, profiles::render_icon},
    room_state::RoomStateStore,
    theme::DeplaceThings,
    watch_bridge::execute_on_change,
};

pub struct ChatInputBar {
    pub chat_input: Entity<InputState>,
    active_room: Receiver<Option<Room>>,
    membership_map: Receiver<MembershipMap>,

    room_store: RoomStateStore,
}

impl ChatInputBar {
    pub fn new(
        state: &AppState,
        window: &mut Window,
        cx: &mut Context<Self>,
        room_store: RoomStateStore,
    ) -> Self {
        let active_room = state.active_room();
        let membership_map = state.membership_map();

        let chat_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(1, 10)
                .placeholder(
                    active_room
                        .borrow()
                        .clone()
                        .get_input_placeholder(&membership_map.borrow()),
                )
        });

        let view = Self {
            chat_input: chat_input.clone(),
            active_room: active_room.clone(),
            membership_map: membership_map.clone(),

            room_store,
        };

        cx.subscribe(&chat_input, |view: &mut Self, chat_input, event, cx| {
            if let InputEvent::Change = event
                && let Some(room) = view.active_room.borrow().clone()
            {
                view.room_store.entry(room.room_id()).chat_input =
                    chat_input.read(cx).text().to_string().into();
            }
        })
        .detach();

        execute_on_change(
            active_room.clone(),
            cx,
            "chat_input",
            move |view, window, cx, room, _| {
                view.on_room_change(window, cx, room.map(|r| r.room_id().to_owned()))
            },
        );

        view
    }

    fn on_room_change(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        room_id: Option<OwnedRoomId>,
    ) {
        let value = if let Some(room_id) = room_id {
            self.room_store.get(&room_id).unwrap_or_default().chat_input
        } else {
            SharedString::default()
        };

        self.chat_input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            let end = input.text().offset_to_position(input.text().len());
            input.set_cursor_position(end, window, cx);
        });

        self.chat_input.focus_handle(cx).focus(window, cx);

        self.update_placeholder(window, cx);
    }

    fn update_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let active_room = self.active_room.borrow().clone();
        let map = self.membership_map.borrow().clone();

        self.chat_input.update(cx, |input, cx| {
            input.set_placeholder(active_room.get_input_placeholder(&map), window, cx)
        });
    }
}

impl Focusable for ChatInputBar {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.chat_input.focus_handle(cx)
    }
}

pub struct SendMessageEvent {
    pub text: String,
}

impl gpui::EventEmitter<SendMessageEvent> for ChatInputBar {}

impl Render for ChatInputBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        let input_focus_handle = self.chat_input.read(cx).focus_handle(cx);
        let input_focused = input_focus_handle.is_focused(window);

        let (input_bg, input_border) = if input_focused {
            (theme.input.focus_background, theme.input.focused_border)
        } else {
            (theme.input.background, theme.tile.border)
        };

        let icon_size = structure.chat.input_height - structure.small_gap * 2.0;

        tailwind_div!(
            min_h(structure.chat.input_height),
            flex,
            flex_row,
            items_center,
            w_full,
            rounded(structure.inner_border_radius),
            paddings(structure.small_gap),
            text_size(structure.chat.text_size),
            gap(structure.small_gap),
            border_1,
            border_color(input_border),
            bg(input_bg)
        )
        .track_focus(&input_focus_handle)
        .on_action(cx.listener(|this, _event: &SendMessage, _window, cx| {
            let text = this.chat_input.read(cx).text().to_string();
            if !text.trim().is_empty() {
                cx.emit(SendMessageEvent { text });
            }
            this.chat_input.update(cx, |input, cx| {
                input.set_value("", _window, cx);
            })
        }))
        .child(
            tailwind_div!(
                w(icon_size),
                h(icon_size),
                flex,
                items_center,
                justify_center,
                rounded((structure.inner_border_radius + structure.smaller_border_radius) / 2.0),
                cursor_pointer,
                text_color(theme.text.dim),
                hover(bg(theme.solid_hover_bg), text_color(theme.text.normal),)
            )
            .id("chat_file_icon")
            .child(render_icon(
                phosphor_svgs::icon::plus::REGULAR,
                icon_size - structure.small_gap * 2.0,
            )),
        )
        .child(
            Input::new(&self.chat_input)
                .p_0()
                .bg_transparent()
                .border_transparent()
                .text_color(theme.text.normal),
        )
    }
}
