use std::{
    hash::Hash,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    ruma::{MxcUri, OwnedMxcUri, UInt, events::room::MediaSource},
};
use mime_guess::Mime;
use tokio::{runtime::Runtime, sync::watch};

/// Meant to be cloned and passed around
pub type AvatarCache = MediaCache<OwnedMxcUri, gpui::Image>;
// pub type FileCache = MediaCache<String, Vec<u8>>;
pub type ThumbnailCache = MediaCache<(gpui::SharedString, u64, u64), gpui::Image>;
pub type VideoCache = MediaCache<OwnedMxcUri, gpui_video_player::Video>;

#[derive(Clone, Default)]
pub enum MediaState<C> {
    Loading,
    Loaded(Arc<C>),
    #[default]
    Failed,
}

impl<C> MediaState<C> {
    pub fn to_option(&self) -> Option<Arc<C>> {
        match self {
            MediaState::Loaded(arc) => Some(Arc::clone(arc)),
            _ => None,
        }
    }
}

impl<C> From<MediaState<C>> for Option<Arc<C>> {
    fn from(val: MediaState<C>) -> Self {
        val.to_option()
    }
}

#[derive(Clone)]
pub struct MediaCache<T: Hash + Eq, C> {
    client: Client,
    tokio_rt: Arc<Runtime>,
    cache: Arc<DashMap<T, MediaState<C>>>,
    loaded_at: Arc<DashMap<T, Instant>>,
    changed: watch::Sender<()>,
}

impl<T: Hash + Eq + Clone, C> MediaCache<T, C> {
    pub fn new(client: Client, tokio_rt: Arc<Runtime>) -> Self {
        let (changed, _) = watch::channel(());
        Self {
            client,
            tokio_rt,
            cache: Arc::new(DashMap::new()),
            loaded_at: Arc::new(DashMap::new()),
            changed,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<()> {
        self.changed.subscribe()
    }

    pub fn loaded_elapsed(&self, key: &T) -> Option<Duration> {
        self.loaded_at.get(key).map(|at| at.elapsed())
    }
}

impl AvatarCache {
    pub fn get(&self, uri: &MxcUri) -> MediaState<gpui::Image> {
        if let Some(state) = self.cache.get(uri) {
            return state.clone();
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
                    let format = guess_image_format(&bytes)?;
                    let image = gpui::Image::from_bytes(format, bytes.to_vec());
                    Some(MediaState::Loaded(Arc::new(image)))
                })
                .unwrap_or_default();

            store.cache.insert(uri, state);
            if let Err(e) = store.changed.send(()) {
                tracing::error!("Failed to send cache change notification: {e}");
            }
        });

        MediaState::Loading
    }
}

// impl FileCache {
//     pub fn get(&self, source: &MediaSource, source_key: &str) -> MediaState<Vec<u8>> {
//         if let Some(state) = self.cache.get(source_key) {
//             return state.clone();
//         }

//         self.cache
//             .insert(source_key.to_string(), MediaState::Loading);

//         let store = self.clone();
//         let tokio_rt = self.tokio_rt.clone();
//         let key = source_key.to_string();
//         let request = MediaRequestParameters {
//             source: source.clone(),
//             format: MediaFormat::File,
//         };
//         tokio_rt.spawn(async move {
//             let state = match store.client.media().get_media_content(&request, true).await {
//                 Ok(bytes) => MediaState::Loaded(Arc::new(bytes)),
//                 Err(e) => {
//                     tracing::error!("Failed to fetch media: {e}");
//                     MediaState::Failed
//                 }
//             };

//             store.cache.insert(key, state);
//             if let Err(e) = store.changed.send(()) {
//                 tracing::error!("Failed to send cache change notification: {e}");
//             }
//         });

//         MediaState::Loading
//     }
// }

impl VideoCache {
    /// Get a video by mxc uri
    ///
    /// The video will be requested from the client, preferably using the cache.
    ///
    /// # Arguments
    ///
    /// - `uri`: I wonder what this could possibly mean
    ///
    /// - `source`: I actually have no clue
    ///
    /// - `size`: Size of the video being requested, this is part of the event.
    ///
    /// The size is used to determine whether the video should be loaded direcly
    /// into memory or saved as file
    pub fn get(
        &self,
        uri: OwnedMxcUri,
        source: &MediaSource,
        size: u64,
    ) -> MediaState<gpui_video_player::Video> {
        let request = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::File,
        };
        let client = self.client.clone();
        self.tokio_rt.spawn(async move {
            let res = client
                .media()
                .get_media_content(request, true)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to get media: {e}");
                })
                .ok();
            let state: gpui_video_player::Video = res
                .and_then(|bytes| {
                    let video = gpui_video_player::Video::new(uri);
                    MediaState::Loaded(Arc::new(video))
                }) // TODO: directly take the bytes, this here won't work
                .unwrap_or_default();

            if matches!(state, MediaState::Loaded(_)) {
                store.loaded_at.insert(key.clone(), Instant::now());
            }

            store.cache.insert(key, state);
            if let Err(e) = store.changed.send(()) {
                tracing::error!("Failed to send cache change notification: {e}");
            }
        });

        MediaState::Loading
    }
}

impl ThumbnailCache {
    pub fn get(
        &self,
        source: &MediaSource,
        source_key: &gpui::SharedString,
        width: u64,
        height: u64,
    ) -> MediaState<gpui::Image> {
        let key = (source_key.clone(), width, height);

        if let Some(state) = self.cache.get(&key) {
            return state.clone();
        }

        self.cache.insert(key.clone(), MediaState::Loading);

        let store = self.clone();
        let tokio_rt = self.tokio_rt.clone();
        let key = key.clone();
        let request = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::Thumbnail(MediaThumbnailSettings {
                method: matrix_sdk::ruma::media::Method::Scale,
                width: UInt::new_saturating(width),
                height: UInt::new_saturating(height),
                animated: true,
            }),
        };
        tokio_rt.spawn(async move {
            let res = store
                .client
                .media()
                .get_media_content(&request, true)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to get media: {e}");
                })
                .ok();

            let state = res
                .and_then(|bytes| {
                    let format = guess_image_format(&bytes)?;
                    let image = gpui::Image::from_bytes(format, bytes.to_vec());
                    Some(MediaState::Loaded(Arc::new(image)))
                })
                .unwrap_or_default();

            if matches!(state, MediaState::Loaded(_)) {
                store.loaded_at.insert(key.clone(), Instant::now());
            }

            store.cache.insert(key, state);
            if let Err(e) = store.changed.send(()) {
                tracing::error!("Failed to send cache change notification: {e}");
            }
        });

        MediaState::Loading
    }
}

fn guess_image_format(bytes: &[u8]) -> Option<gpui::ImageFormat> {
    if is_svg(bytes) {
        return Some(gpui::ImageFormat::Svg);
    }

    match image::guess_format(bytes) {
        Ok(format) => Some(gpui_format_from(format)),
        Err(e) => {
            tracing::error!("Failed to guess image format: {e}");
            None
        }
    }
}

fn is_svg(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(512)];
    String::from_utf8_lossy(sample).contains("<svg")
}

fn gpui_format_from(format: image::ImageFormat) -> gpui::ImageFormat {
    match format {
        image::ImageFormat::Png => gpui::ImageFormat::Png,
        image::ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        image::ImageFormat::Gif => gpui::ImageFormat::Gif,
        _ => gpui::ImageFormat::Png,
    }
}
