use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use deplace_core::{
    NameExt,
    colors::{Color, ColorExt},
    get_other_member,
    state::MembershipMap,
};
use gpui::{
    AnyElement, App, BoxShadow, Div, Entity, Focusable, Length, ObjectFit, Pixels, Window, div,
    img, prelude::*, px, svg,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};
use matrix_sdk::{
    Client, Room,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    room::RoomMember,
    ruma::{MxcUri, OwnedMxcUri, UInt, UserId, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::theme::AppTheme;

pub mod root;

mod chat;
mod discovery;
mod dm_list;
mod header;
mod home;
mod login;
mod quick_select;
mod server_list;
mod sidebar;
mod timeline;

pub fn floating_tile(theme: &AppTheme) -> Div {
    div()
        .flex()
        .backdrop_blur(px(30.0))
        .flex_shrink_0()
        .bg(theme.tile.background)
        .border(theme.tile.border_thickness)
        .border_color(theme.tile.border)
        .rounded(theme.tile.border_radius)
        .gap(theme.gap)
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

pub type AvatarCache = MediaCache<OwnedMxcUri>;

#[derive(Clone)]
enum MediaState {
    Loading,
    Loaded(Arc<Vec<u8>>),
    Failed,
}

#[derive(Clone)]
pub struct MediaCache<T> {
    client: Client,
    tokio_rt: Arc<Runtime>,
    cache: Arc<RwLock<HashMap<T, MediaState>>>,
    changed: watch::Sender<()>,
}

impl<T> MediaCache<T> {
    pub fn new(client: Client, tokio_rt: Arc<Runtime>) -> Self {
        let (changed, _) = watch::channel(());
        Self {
            client,
            tokio_rt,
            cache: Arc::new(RwLock::new(HashMap::new())),
            changed,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }
}

impl MediaCache<OwnedMxcUri> {
    pub fn get(&self, uri: &MxcUri) -> Option<Arc<Vec<u8>>> {
        {
            let cache = self
                .cache
                .read()
                .unwrap_or_else(|poison| poison.into_inner());
            match cache.get(uri) {
                Some(MediaState::Loaded(bytes)) => return Some(bytes.clone()),
                Some(_) => return None, // already Loading/Failed
                None => {}
            }
        }

        self.cache
            .write()
            .unwrap()
            .insert(uri.to_owned(), MediaState::Loading);

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
            let state = match store.client.media().get_media_content(&request, true).await {
                Ok(bytes) => MediaState::Loaded(Arc::new(bytes)),
                Err(e) => {
                    tracing::error!("Failed to fetch media {uri}: {e}");
                    MediaState::Failed
                }
            };
            store.cache.write().unwrap().insert(uri, state);
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
    image_bytes: Option<Arc<Vec<u8>>>,
) -> AnyElement {
    if let Some(image_bytes) = image_bytes {
        let format = match image::guess_format(&image_bytes) {
            Ok(format) => format,
            Err(e) => {
                tracing::error!("Failed to guess image format: {:?}", e);
                return text_circle(initial, color, size, rounding);
            }
        };
        let image = Arc::new(gpui::Image::from_bytes(
            gpui_format_from(format),
            image_bytes.to_vec(),
        ));
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
        render_icon(phosphor_svgs::icon::hash::BOLD, size)
    } else {
        render_icon(phosphor_svgs::icon::speaker_high::FILL, size)
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
    let image_bytes = url.and_then(|url| cache.get(&url));

    avatar(room.initial(), room.color(), size, rounding, image_bytes)
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
