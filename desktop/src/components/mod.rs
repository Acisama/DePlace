use blurhash::decode;
use deplace_core::formatting::format_bytes;
use deplace_core::settings::DataSizeUnit;
use gpui::{
    App, BoxShadow, Div, ElementId, Entity, Focusable, Hsla, Pixels, SharedString, Window,
    prelude::*, transparent_black,
};
use gpui::{RenderImage, Role};
use gpui_component::tooltip::Tooltip;
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};
use image::{Frame, ImageBuffer, Rgba};
use macros::tailwind_div;
use smallvec::SmallVec;
use std::sync::Arc;

use crate::components::profiles::render_icon;
use crate::theme::DeplaceThings;
use crate::theme::{AppTheme, Structure};

pub mod chat;
pub mod message;
pub mod profiles;
pub mod root;

mod discovery;
mod header;
mod home;
mod login;
mod overlay;
mod quick_select;
mod server_list;
mod settings;
mod sidebar;
mod verification;

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

pub fn input(entity: &Entity<InputState>, window: &Window, cx: &App, role: Role) -> Input {
    let theme = cx.app_theme();
    let structure = cx.structure();

    let focused = entity.read(cx).focus_handle(cx).is_focused(window);
    let (bg, border) = if focused {
        (theme.input.focus_background, theme.input.focused_border)
    } else {
        (theme.input.background, theme.tile.border)
    };

    Input::new(entity)
        .role(role)
        .paddings(structure.small_gap)
        .text_color(theme.text.normal)
        .bg(bg)
        .border_1()
        .cleanable(true)
        .border_color(border)
}

pub trait CustomStyles: Styled + Sized {
    fn border_transparent(self) -> Self {
        self.border_1().border_color(transparent_black())
    }

    fn bg_transparent(self) -> Self {
        self.bg(transparent_black())
    }

    fn outer_gradient(self, color: Hsla, size: Pixels) -> Self {
        self.text_color(color).shadow(vec![BoxShadow {
            color,
            blur_radius: size,
            inset: true,
            offset: Default::default(),
            spread_radius: size,
        }])
    }
}

impl<T: Styled> CustomStyles for T {}

#[derive(Clone)]
pub struct ByteSize {
    _bytes: u64,
    bytes_str: SharedString,
    bits_str: SharedString,
    mibi_bytes_str: SharedString,
}

impl ByteSize {
    pub fn new(bytes: u64) -> Self {
        Self {
            _bytes: bytes,
            bits_str: format_bytes(bytes, DataSizeUnit::Bits).into(),
            bytes_str: format_bytes(bytes, DataSizeUnit::Bytes).into(),
            mibi_bytes_str: format_bytes(bytes, DataSizeUnit::Mibibytes).into(),
        }
    }

    pub fn get(&self, format: &DataSizeUnit) -> SharedString {
        match format {
            DataSizeUnit::Bytes => self.bytes_str.clone(),
            DataSizeUnit::Bits => self.bits_str.clone(),
            DataSizeUnit::Mibibytes => self.mibi_bytes_str.clone(),
        }
    }
}

pub fn blurhash_to_image(hash: &str) -> Option<Arc<RenderImage>> {
    let width = 32;
    let height = 32;
    let mut pixels = match decode(hash, width, height, 1.2) {
        Ok(pixels) => pixels,
        Err(e) => {
            eprintln!("Failed to decode blurhash: {:?}", e);
            return None;
        }
    };

    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    let buf = match ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, pixels) {
        Some(buf) => buf,
        None => {
            eprintln!("Failed to construct ImageBuffer");
            return None;
        }
    };

    Some(Arc::new(RenderImage::new(SmallVec::from_elem(
        Frame::new(buf),
        1,
    ))))
}

pub fn close_button(
    theme: &AppTheme,
    id: impl Into<ElementId>,
    size: Pixels,
) -> gpui::Stateful<Div> {
    tailwind_div!(
        border_transparent,
        text_color(theme.text.dim),
        hover(
            bg(theme.solid_bg),
            border_color(theme.tile.border),
            text_color(theme.text.normal)
        )
    )
    .cursor_pointer()
    .child(render_icon(phosphor_svgs::icon::x::REGULAR, size))
    .id(id)
}

pub trait TooltipExt<T> {
    fn custom_tooltip(self, text: &'static str, theme: &AppTheme, structure: &Structure) -> T;
}

impl<T: StatefulInteractiveElement> TooltipExt<T> for T {
    fn custom_tooltip(self, text: &'static str, theme: &AppTheme, structure: &Structure) -> T {
        self.tooltip({
            let text_normal = theme.text.normal;
            let border_color = theme.tile.border;
            let bg_color = theme.solid_bg;
            let inner = structure.semi_border_radius();

            move |window: &mut Window, cx| {
                Tooltip::new(text)
                    .text_color(text_normal)
                    .border_color(border_color)
                    .rounded(inner)
                    .bg(bg_color)
                    .build(window, cx)
            }
        })
    }
}
