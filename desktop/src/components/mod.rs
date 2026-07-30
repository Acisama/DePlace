use gpui::{App, Div, Entity, Focusable, Window, div, prelude::*};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};

use crate::theme::AppTheme;

pub mod discovery;
pub mod home;
pub mod login;
pub mod root;

pub fn floating_tile(theme: &AppTheme) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .bg(theme.tile.background)
        .border(theme.tile.border_thickness)
        .border_color(theme.tile.border)
        .rounded(theme.tile.border_radius)
        .gap(theme.tile.gap)
        .paddings(theme.tile.padding)
        .shadow_sm()
        .overflow_y_hidden()
}

pub fn input(theme: &AppTheme, entity: &Entity<InputState>, window: &Window, cx: &App) -> Input {
    let focused = entity.read(cx).focus_handle(cx).is_focused(window);
    let (bg, border) = if focused {
        (theme.input.focus_background, theme.input.focused_border)
    } else {
        (theme.input.background, theme.tile.border)
    };

    Input::new(entity)
        .paddings(theme.input.padding)
        .text_color(theme.text.normal)
        .bg(bg)
        .border_1()
        .cleanable(true)
        .border_color(border)
}
