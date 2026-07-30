use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use deplace_core::colors::Color;
use gpui::{
    AnyElement, App, BoxShadow, Div, Entity, Focusable, Image, ObjectFit, Pixels, Window, div, img,
    prelude::*, px,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
};
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters},
    ruma::{OwnedMxcUri, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

use crate::theme::AppTheme;

pub mod discovery;
pub mod dm_list;
pub mod home;
pub mod login;
pub mod root;
pub mod server_list;

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
                format: MediaFormat::File,
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
        .child(initial.to_string())
        .child(
            div()
                .absolute()
                .rounded(rounding)
                .inset_0()
                .shadow(vec![BoxShadow {
                    color: color.to_gpui(),
                    blur_radius: px(4.0),
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
            .into_any()
    } else {
        text_circle(initial, color, size, rounding)
    }
}
