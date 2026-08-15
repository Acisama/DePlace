use deplace_core::{
    NameExt,
    colors::{Color, ColorExt},
    get_other_member,
    matrix_api::presence::PresenceIcon,
    state::{MembershipMap, PresenceMap},
};
use gpui::{AnyElement, Div, Hsla, Length, ObjectFit, Pixels, div, img, prelude::*, relative, svg};
use gpui_component::{Colorize, StyledExt};
use macros::tailwind_div;
use matrix_sdk::{Room, room::RoomMember, ruma::UserId};

use crate::{
    components::{
        CustomStyles,
        cache::{AvatarCache, MediaState},
    },
    theme::{AppTheme, Colors},
};

pub fn text_circle(initial: char, color: Hsla, size: Pixels, rounding: Pixels) -> Div {
    let font_size = size / 2.0;

    let bg_color = color.lightness(0.1);

    tailwind_div!(
        bg(bg_color),
        relative,
        rounded(rounding),
        size(size),
        flex,
        font_bold,
        text_size(font_size),
        text_color(color),
        items_center,
        justify_center,
        text_center,
        cursor_pointer,
        outer_gradient(color, size / 17.0)
    )
    .child(initial.to_string())
}

fn avatar(
    initial: char,
    color: Hsla,
    size: Pixels,
    rounding: Pixels,
    image: Option<MediaState<gpui::Image>>,
) -> Div {
    match image {
        Some(MediaState::Loaded(image)) => tailwind_div!(
            rounded(rounding),
            size(size),
            items_center,
            justify_center,
            overflow_hidden,
            cursor_pointer
        )
        .child(
            img(image)
                .object_fit(ObjectFit::Cover)
                .size_full()
                .rounded(rounding),
        ),
        Some(MediaState::Failed) => text_circle('!', color, size, rounding),
        Some(MediaState::Loading) => text_circle(initial, color, size, rounding),
        None => text_circle(initial, color, size, rounding),
    }
}

fn unknown_avatar(size: Pixels, rounding: Pixels, color: Hsla) -> Div {
    avatar('?', color, size, rounding, Some(MediaState::Failed))
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

#[allow(clippy::too_many_arguments)]
fn render_room_avatar(
    room: &Room,
    map: &MembershipMap,
    presence_map: Option<&PresenceMap>,
    own_id: &UserId,
    cache: &AvatarCache,
    size: Pixels,
    rounding: Pixels,
    theme: &AppTheme,
) -> AnyElement {
    if room.is_dm() {
        let other_member = get_other_member(own_id, map, room.room_id());
        if let Some(other_member) = other_member {
            return other_member.render_avatar(size, rounding, theme, cache, presence_map);
        }
    }

    render_room_no_dm(room, cache, size, rounding, false)
}

pub fn render_room_no_dm(
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
    let image = url.map(|url| cache.get(&url));

    avatar(room.initial(), room.color().into(), size, rounding, image).into_any()
}

#[allow(clippy::too_many_arguments)]
pub fn render_room_icon(
    room: &Room,
    map: &MembershipMap,
    presences: Option<&PresenceMap>,
    own_id: &UserId,
    cache: &AvatarCache,
    size: Pixels,
    rounding: Pixels,
    theme: &AppTheme,
) -> AnyElement {
    if room.is_dm() {
        render_room_avatar(room, map, presences, own_id, cache, size, rounding, theme)
    } else {
        render_simple_room_icon(room, size)
    }
}

pub trait MemberRenderer {
    fn render_avatar(
        &self,
        size: Pixels,
        rounding: Pixels,
        theme: &AppTheme,
        cache: &AvatarCache,
        presence_map: Option<&PresenceMap>,
    ) -> AnyElement;
    fn render_name(&self, size: Pixels, colors: &Colors) -> AnyElement;
}

fn render_name(name: String, color: Hsla, size: Pixels) -> Div {
    div()
        .text_color(color)
        .font_semibold()
        .text_size(size)
        .line_height(relative(1.0))
        .child(name)
}

fn render_unknown_name(size: Pixels, color: Hsla) -> Div {
    render_name("Unknown".to_string(), color, size)
}

impl MemberRenderer for RoomMember {
    fn render_avatar(
        &self,
        size: Pixels,
        rounding: Pixels,
        theme: &AppTheme,
        cache: &AvatarCache,
        presence_map: Option<&PresenceMap>,
    ) -> AnyElement {
        let image = self.avatar_url().map(|url| cache.get(url));

        let content = avatar(self.initial(), self.color().into(), size, rounding, image).into_any();

        if let Some(map) = presence_map {
            let icon_size = size * 0.3;
            let padding = icon_size * 0.35;

            let icon: PresenceIcon = map.get(self.user_id()).into();

            let color = match icon {
                PresenceIcon::Online => theme.colors.online,
                PresenceIcon::Offline => theme.colors.offline,
                PresenceIcon::Busy => theme.colors.busy,
                PresenceIcon::Idle => theme.colors.idle,
            };

            tailwind_div!(relative, size(size))
                .child(content)
                .child(
                    tailwind_div!(
                        absolute,
                        paddings(padding),
                        right(-padding),
                        bottom(-padding),
                        rounded_full,
                        bg(theme.solid_bg)
                    )
                    .child(
                        svg()
                            .path(icon.icon_path())
                            .size(icon_size)
                            .text_color(color),
                    ),
                )
                .into_any()
        } else {
            content
        }
    }

    fn render_name(&self, size: Pixels, _colors: &Colors) -> AnyElement {
        let color: Color = self.color();
        render_name(self.get_name(), color.into(), size).into_any()
    }
}

impl MemberRenderer for Option<&RoomMember> {
    fn render_avatar(
        &self,
        size: Pixels,
        rounding: Pixels,
        theme: &AppTheme,
        cache: &AvatarCache,
        presence_map: Option<&PresenceMap>,
    ) -> AnyElement {
        if let Some(member) = self {
            member.render_avatar(size, rounding, theme, cache, presence_map)
        } else {
            unknown_avatar(size, rounding, theme.colors.unknown).into_any()
        }
    }

    fn render_name(&self, size: Pixels, colors: &Colors) -> AnyElement {
        if let Some(member) = self {
            member.render_name(size, colors).into_any()
        } else {
            render_unknown_name(size, colors.unknown).into_any()
        }
    }
}
