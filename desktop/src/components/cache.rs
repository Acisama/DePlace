use std::{hash::Hash, sync::Arc};

use dashmap::DashMap;
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings, UniqueKey},
    ruma::{MxcUri, OwnedMxcUri, UInt, events::room::MediaSource},
};
use tokio::{runtime::Runtime, sync::watch};

pub type AvatarCache = MediaCache<OwnedMxcUri, gpui::Image>;
pub type FileCache = MediaCache<String, Vec<u8>>;
pub type ImageCache = MediaCache<String, gpui::Image>;

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

impl AvatarCache {
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
            if let Err(e) = store.changed.send(()) {
                tracing::error!("Failed to send cache change notification: {e}");
            }
        });

        None
    }
}

impl FileCache {
    pub fn get(&self, request: &MediaRequestParameters) -> Option<Arc<Vec<u8>>> {
        let key = request.unique_key();

        if let Some(state) = self.cache.get(&key) {
            return match &*state {
                MediaState::Loaded(bytes) => Some(bytes.clone()),
                _ => None, // Loading or Failed
            };
        }

        self.cache.insert(key.clone(), MediaState::Loading);

        let store = self.clone();
        let tokio_rt = self.tokio_rt.clone();
        let key = key.clone();
        let request = request.clone();
        tokio_rt.spawn(async move {
            let state = match store.client.media().get_media_content(&request, true).await {
                Ok(bytes) => MediaState::Loaded(Arc::new(bytes)),
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    MediaState::Failed
                }
            };

            store.cache.insert(key, state);
            if let Err(e) = store.changed.send(()) {
                tracing::error!("Failed to send cache change notification: {e}");
            }
        });

        None
    }
}

impl ImageCache {
    pub fn get(&self, request: &MediaRequestParameters) -> Option<Arc<gpui::Image>> {
        let key = request.unique_key();

        if let Some(state) = self.cache.get(&key) {
            return match &*state {
                MediaState::Loaded(bytes) => Some(bytes.clone()),
                _ => None, // Loading or Failed
            };
        }

        self.cache.insert(key.clone(), MediaState::Loading);

        let store = self.clone();
        let tokio_rt = self.tokio_rt.clone();
        let key = key.clone();
        let request = request.clone();
        tokio_rt.spawn(async move {
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

            store.cache.insert(key, state);
            if let Err(e) = store.changed.send(()) {
                tracing::error!("Failed to send cache change notification: {e}");
            }
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
