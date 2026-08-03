use std::sync::Arc;

use deplace_core::{
    NameExt,
    colors::{Color, ColorExt},
    formatting::{DataSizeUnit, format_bytes},
    get_other_member,
    state::MembershipMap,
};
use gpui::{
    AnyElement, App, BoxShadow, Div, Entity, Focusable, Length, ObjectFit, Pixels, SharedString,
    Window, div, img, prelude::*, px, relative, svg, transparent_black,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};
use macros::tailwind_div;
use matrix_sdk::{Room, room::RoomMember, ruma::UserId};

use crate::{
    components::cache::AvatarCache,
    theme::{ActiveAppTheme, AppTheme, Structure, StructureExt},
};

pub mod cache;
pub mod root;

mod chat;
mod discovery;
mod dm_list;
mod header;
mod home;
mod login;
mod message;
mod quick_select;
mod server_list;
mod sidebar;

pub fn floating_tile(theme: &AppTheme, structure: &Structure) -> Div {
    tailwind_div!(
        flex,
        backdrop_blur(theme.blur),
        flex_shrink_0,
        bg(theme.tile.background),
        border_1(),
        border_color(theme.tile.border),
        rounded(structure.outer_border_radius),
        gap(structure.gap),
        shadow_sm(),
        overflow_y_hidden()
    )
}

pub fn input(entity: &Entity<InputState>, window: &Window, cx: &App) -> Input {
    let theme = cx.app_theme();
    let structure = cx.structure();

    let focused = entity.read(cx).focus_handle(cx).is_focused(window);
    let (bg, border) = if focused {
        (theme.input.focus_background, theme.input.focused_border)
    } else {
        (theme.input.background, theme.tile.border)
    };

    Input::new(entity)
        .paddings(structure.small_gap)
        .text_color(theme.text.normal)
        .bg(bg)
        .border_1()
        .cleanable(true)
        .border_color(border)
}

pub fn text_circle(initial: char, color: Color, size: Pixels, rounding: Pixels) -> AnyElement {
    let font_size = size / 2.0;

    let bg_color = color.set_lightness(0.1);

    div()
        .bg(bg_color.to_gpui())
        .relative()
        .rounded(rounding)
        .size(size)
        .flex()
        .font_bold()
        .text_size(font_size)
        .text_color(color.to_gpui())
        .items_center()
        .justify_center()
        .text_center()
        .cursor_pointer()
        .child(initial.to_string())
        .child(
            div()
                .absolute()
                .rounded(rounding)
                .inset_0()
                .shadow(vec![BoxShadow {
                    color: color.to_gpui(),
                    blur_radius: px(2.0),
                    inset: true,
                    offset: Default::default(),
                    spread_radius: px(2.0),
                }]),
        )
        .into_any()
}

fn avatar(
    initial: char,
    color: Color,
    size: Pixels,
    rounding: Pixels,
    image: Option<Arc<gpui::Image>>,
) -> AnyElement {
    if let Some(image) = image {
        img(image)
            .object_fit(ObjectFit::Cover)
            .rounded(rounding)
            .size(size)
            .items_center()
            .justify_center()
            .overflow_hidden()
            .cursor_pointer()
            .into_any()
    } else {
        text_circle(initial, color, size, rounding)
    }
}

fn unknown_avatar(size: Pixels, rounding: Pixels) -> AnyElement {
    avatar('?', Color::UNKNOWN, size, rounding, None)
}

pub fn render_icon(svg_content: &'static str, size: impl Clone + Into<Length>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .child(svg().source(svg_content.as_bytes()).size(size))
        .into_any()
}

pub fn render_simple_room_icon(room: &Room, size: impl Clone + Into<Length>) -> AnyElement {
    if room.is_call() {
        render_icon(phosphor_svgs::icon::speaker_high::FILL, size)
    } else {
        render_icon(phosphor_svgs::icon::hash::BOLD, size)
    }
}

fn render_room_avatar(
    room: &Room,
    map: &MembershipMap,
    own_id: &UserId,
    cache: &AvatarCache,
    size: Pixels,
    rounding: Pixels,
) -> AnyElement {
    if room.is_dm() {
        let other_member = get_other_member(own_id, map, room.room_id());
        if let Some(other_member) = other_member {
            return other_member.render_avatar(size, rounding, cache);
        }
    }

    render_room_no_dm(room, cache, size, rounding, false)
}

fn render_room_no_dm(
    room: &Room,
    cache: &AvatarCache,
    size: Pixels,
    rounding: Pixels,
    warn: bool,
) -> AnyElement {
    if room.is_dm() && warn {
        tracing::warn!("Rendering without dm, but room {} is a dm", room.room_id());
    }

    let url = room.avatar_url().map(|u| u.to_owned());
    let image = url.and_then(|url| cache.get(&url));

    avatar(room.initial(), room.color(), size, rounding, image)
}

fn render_room_icon(
    room: &Room,
    map: &MembershipMap,
    own_id: &UserId,
    cache: &AvatarCache,
    size: Pixels,
    rounding: Pixels,
) -> AnyElement {
    if room.is_dm() {
        render_room_avatar(room, map, own_id, cache, size, rounding)
    } else {
        render_simple_room_icon(room, size)
    }
}

pub trait MemberRenderer {
    fn render_avatar(&self, size: Pixels, rounding: Pixels, cache: &AvatarCache) -> AnyElement;
    fn render_name(&self, size: Pixels) -> Div;
}

fn render_name(name: String, color: Color, size: Pixels) -> Div {
    div()
        .text_color(color.to_gpui())
        .font_bold()
        .text_size(size)
        .line_height(relative(1.0))
        .child(name)
}

fn render_unknown_name(size: Pixels) -> Div {
    render_name("Unknown".to_string(), Color::UNKNOWN, size)
}

impl MemberRenderer for RoomMember {
    fn render_avatar(&self, size: Pixels, rounding: Pixels, cache: &AvatarCache) -> AnyElement {
        let image = self.avatar_url().and_then(|url| cache.get(url));
        avatar(self.initial(), self.color(), size, rounding, image)
    }

    fn render_name(&self, size: Pixels) -> Div {
        let color: Color = self.color();
        render_name(self.get_name(), color, size)
    }
}

impl MemberRenderer for Option<&RoomMember> {
    fn render_avatar(&self, size: Pixels, rounding: Pixels, cache: &AvatarCache) -> AnyElement {
        if let Some(member) = self {
            member.render_avatar(size, rounding, cache)
        } else {
            unknown_avatar(size, rounding)
        }
    }

    fn render_name(&self, size: Pixels) -> Div {
        if let Some(member) = self {
            member.render_name(size)
        } else {
            render_unknown_name(size)
        }
    }
}

pub trait CustomStyles: Styled + Sized {
    fn border_transparent(self) -> Self {
        self.border_1().border_color(transparent_black())
    }
}

impl<T: gpui::Styled> CustomStyles for T {}

#[derive(Clone)]
pub struct ByteSize {
    bytes: u64,
    bytes_str: SharedString,
    bits_str: SharedString,
    mibi_bytes_str: SharedString,
}

impl ByteSize {
    pub fn new(bytes: u64) -> Self {
        Self {
            bytes,
            bits_str: format_bytes(bytes, DataSizeUnit::Bits).into(),
            bytes_str: format_bytes(bytes, DataSizeUnit::Bytes).into(),
            mibi_bytes_str: format_bytes(bytes, DataSizeUnit::MibiBytes).into(),
        }
    }
}
