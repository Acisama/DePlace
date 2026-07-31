use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use deplace_core::{
    colors::{Color, UNKNOWN_COLOR},
    get_dm_room_name, get_other_member, get_room_name,
    state::MembershipMap,
};
use gpui::{
    AnyElement, App, BoxShadow, Div, Entity, Focusable, Image, Length, ObjectFit, Pixels, Window,
    div, img, prelude::*, px, svg,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};
use matrix_sdk::{
    Client, Room,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    ruma::{OwnedMxcUri, UInt, UserId, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::theme::AppTheme;

pub mod root;

mod discovery;
mod dm_list;
mod header;
mod home;
mod login;
mod quick_select;
mod server_list;
mod sidebar;

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
    pub fn get(&self, source: &MediaSource) -> Option<Arc<Vec<u8>>> {
        let uri = match source {
            MediaSource::Plain(uri) => uri.clone(),
            MediaSource::Encrypted(file) => file.url.clone(),
        };

        {
            let cache = self.cache.read().unwrap();
            match cache.get(&uri) {
                Some(MediaState::Loaded(bytes)) => return Some(bytes.clone()),
                Some(_) => return None, // already Loading/Failed
                None => {}
            }
        }

        self.cache
            .write()
            .unwrap()
            .insert(uri.clone(), MediaState::Loading);

        let store = self.clone();
        let source = source.clone();
        let tokio_rt = self.tokio_rt.clone();
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

pub fn avatar(
    initial: char,
    color: Color,
    size: Pixels,
    rounding: Pixels,
    image: Option<Arc<Image>>,
) -> AnyElement {
    if let Some(image) = image {
        img(image)
            .object_fit(ObjectFit::Cover)
            .rounded(rounding)
            .size(size)
            .cursor_pointer()
            .into_any()
    } else {
        text_circle(initial, color, size, rounding)
    }
}

pub fn render_icon(svg_content: &'static str, size: impl Clone + Into<Length>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .child(svg().source(svg_content.as_bytes()).size(size))
        .into_any()
}

pub fn render_room_icon(room: &Room, size: impl Clone + Into<Length>) -> AnyElement {
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
    let (url, color): (Option<OwnedMxcUri>, Color) = if room.is_dm() {
        let Some(other_member) = get_other_member(own_id, map, room.room_id()) else {
            return avatar(' ', UNKNOWN_COLOR.into(), px(24.0), px(4.0), None).into_any();
        };
        (
            other_member.avatar_url().map(|u| u.to_owned()),
            other_member.into(),
        )
    } else {
        (room.avatar_url().map(|u| u.to_owned()), room.into())
    };

    let image = url.and_then(|url| {
        let source = MediaSource::Plain(url);
        let bytes = cache.get(&source)?;
        let format = image::guess_format(&bytes).ok()?;
        let image = Arc::new(gpui::Image::from_bytes(
            gpui_format_from(format),
            bytes.to_vec(),
        ));
        Some(image)
    });

    let name = if room.is_dm() {
        get_dm_room_name(room, map, own_id)
    } else {
        get_room_name(room)
    };

    avatar(
        name.chars().next().unwrap_or(' '),
        color,
        size,
        rounding,
        image,
    )
}
