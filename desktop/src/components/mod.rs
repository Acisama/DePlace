use std::{hash::Hash, sync::Arc};

use dashmap::DashMap;
use deplace_core::{
    NameExt,
    colors::{Color, ColorExt},
    get_other_member,
    state::MembershipMap,
};
use gpui::{
    AnyElement, App, BoxShadow, Div, Entity, Focusable, Length, ObjectFit, Pixels, Window, div,
    img, prelude::*, px, relative, svg, transparent_black,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};
use macros::tailwind_div;
use matrix_sdk::{
    Client, Room,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    room::RoomMember,
    ruma::{MxcUri, OwnedMxcUri, UInt, UserId, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::theme::{ActiveAppTheme, AppTheme, Structure, StructureExt};

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

pub type AvatarCache = MediaCache<OwnedMxcUri, gpui::Image>;

#[derive(Clone)]
enum MediaState<C> {
    Loading,
    Loaded(Arc<C>),
    Failed,
}

#[derive(Clone)]
pub struct MediaCache<T: Hash + Eq, C> {
    client: Client,
    tokio_rt: Arc<Runtime>,
    cache: Arc<DashMap<T, MediaState<C>>>,
    changed: watch::Sender<()>,
}

impl<T: Hash + Eq, C> MediaCache<T, C> {
    pub fn new(client: Client, tokio_rt: Arc<Runtime>) -> Self {
        let (changed, _) = watch::channel(());
        Self {
            client,
            tokio_rt,
            cache: Arc::new(DashMap::new()),
            changed,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }
}

impl MediaCache<OwnedMxcUri, gpui::Image> {
    pub fn get(&self, uri: &MxcUri) -> Option<Arc<gpui::Image>> {
        if let Some(state) = self.cache.get(uri) {
            return match &*state {
                MediaState::Loaded(img) => Some(img.clone()),
                _ => None, // Loading or Failed
            };
        }

        self.cache.insert(uri.to_owned(), MediaState::Loading);

        let store = self.clone();
        let source = MediaSource::Plain(uri.to_owned());
        let tokio_rt = self.tokio_rt.clone();
        let uri = uri.to_owned();
        tokio_rt.spawn(async move {
            let request = MediaRequestParameters {
                source,
                format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(
                    UInt::new_saturating(100),
                    UInt::new_saturating(100),
                )),
            };

            let res = store
                .client
                .media()
                .get_media_content(&request, true)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to fetch media: {e}");
                })
                .ok();

            let state = res
                .and_then(|bytes| {
                    let format = match image::guess_format(&bytes) {
                        Ok(format) => format,
                        Err(e) => {
                            tracing::error!("Failed to guess image format: {:?}", e);
                            return None;
                        }
                    };
                    let image = gpui::Image::from_bytes(gpui_format_from(format), bytes.to_vec());
                    Some(MediaState::Loaded(Arc::new(image)))
                })
                .unwrap_or(MediaState::Failed);

            store.cache.insert(uri, state);
            let _ = store.changed.send(());
        });

        None
    }
}

fn gpui_format_from(format: image::ImageFormat) -> gpui::ImageFormat {
    match format {
        image::ImageFormat::Png => gpui::ImageFormat::Png,
        image::ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        image::ImageFormat::Gif => gpui::ImageFormat::Gif,
        _ => gpui::ImageFormat::Png,
    }
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
