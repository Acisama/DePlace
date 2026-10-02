use std::{hash::Hash, sync::Arc};

use dashmap::DashMap;
use matrix_sdk::Client;
use ruma::OwnedMxcUri;
use ruma::events::room::MediaSource;

#[derive(Default, Debug)]
pub enum MediaState<T> {
    #[default]
    Loading,
    Failed,
    Loaded(Arc<T>),
}

impl<T> MediaState<T> {
    fn loaded(thing: T) -> Self {
        MediaState::Loaded(Arc::new(thing))
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, MediaState::Loading)
    }
}

impl<T> Clone for MediaState<T> {
    fn clone(&self) -> Self {
        match self {
            MediaState::Loading => MediaState::Loading,
            MediaState::Failed => MediaState::Failed,
            MediaState::Loaded(arc) => MediaState::Loaded(arc.clone()),
        }
    }
}

impl<T> std::hash::Hash for MediaState<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            MediaState::Loading => 0.hash(state),
            MediaState::Failed => 1.hash(state),
            MediaState::Loaded(_) => 2.hash(state),
        }
    }
}

/// Extention trait for caches for loading content where the source can be used to  generate the key
pub trait CacheLoadingExt<T> {
    fn load_content(
        &self,
        source: &T,
    ) -> impl std::future::Future<Output = (MediaLoaded, bool)> + Send;
}

/// Extention trait for caches for loading content where the key cannot be generated from the source and must be provided separately
pub trait CacheLoadingWithKeyExt<T, K> {
    fn load_content_with_key(
        &self,
        source: &T,
        key: K,
    ) -> impl std::future::Future<Output = (MediaLoaded, bool)> + Send;
}

#[derive(Clone, Debug)]
pub struct MediaCache<T: Hash + Eq, C> {
    client: Client,
    cache: Arc<DashMap<T, MediaState<C>>>,
}

impl<T: Hash + Eq, C> MediaCache<T, C> {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            cache: Arc::new(DashMap::new()),
        }
    }

    pub fn get(&self, key: &T) -> Option<MediaState<C>> {
        self.cache.get(key).map(|v| (*v).clone())
    }
}

#[derive(Clone, Debug)]
pub enum NeedsMedia {
    Thumbnail {
        source: MediaSource,
        key: (String, u64, u64),
    },
    Avatar {
        uri: OwnedMxcUri,
    },
    Video {
        source: MediaSource,
    },
}

impl NeedsMedia {
    pub fn avatar(uri: OwnedMxcUri) -> Self {
        Self::Avatar { uri }
    }

    pub fn thumbnail(source: MediaSource, key: (String, u64, u64)) -> Self {
        Self::Thumbnail { source, key }
    }

    pub fn video(source: MediaSource) -> Self {
        Self::Video { source }
    }
}

#[derive(Clone, Debug)]
pub enum MediaLoaded {
    Thumbnail { key: (String, u64, u64) },
    Avatar { uri: OwnedMxcUri },
    Video { key: String },
}

#[cfg(feature = "iced_desktop")]
pub use iced_caches::{AvatarCache, ThumbnailCache, VideoCache};

#[cfg(feature = "iced_desktop")]
mod iced_caches {
    use std::sync::Arc;

    use iced_video_player::Video;
    use matrix_sdk::media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings};
    use ruma::{OwnedMxcUri, UInt, events::room::MediaSource, uint};

    use iced::widget::image::Handle as ImageHandle;

    use super::{CacheLoadingExt, CacheLoadingWithKeyExt, MediaCache, MediaLoaded, MediaState};

    pub type AvatarCache = MediaCache<OwnedMxcUri, iced::widget::image::Handle>;

    impl CacheLoadingExt<OwnedMxcUri> for AvatarCache {
        /// Loads an avatar with the given URI and returns MediaLoaded and a boolean indicating whether it was successfully loaded.
        async fn load_content(&self, uri: &OwnedMxcUri) -> (MediaLoaded, bool) {
            let media_res = MediaLoaded::Avatar { uri: uri.clone() };

            if self.cache.get(uri).is_some() {
                return (media_res, true);
            }

            self.cache.insert(uri.clone(), MediaState::Loading);

            let request = MediaRequestParameters {
                source: MediaSource::Plain(uri.clone()),
                format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(uint!(100), uint!(100))),
            };

            let mut success = true;
            let res = match self.client.media().get_media_content(&request, true).await {
                Ok(bytes) => MediaState::loaded(iced::widget::image::Handle::from_bytes(bytes)),
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    success = false;
                    MediaState::Failed
                }
            };

            self.cache.insert(uri.clone(), res);

            (media_res, success)
        }
    }

    pub type ThumbnailCache = MediaCache<(String, u64, u64), iced::widget::image::Handle>;

    impl CacheLoadingWithKeyExt<MediaSource, (String, u64, u64)> for ThumbnailCache {
        /// Loads a thumbnail with the given key (incldues it's size) and returns MediaLoaded and a boolean indicating whether it was successfully loaded.
        async fn load_content_with_key(
            &self,
            source: &MediaSource,
            key: (String, u64, u64),
        ) -> (MediaLoaded, bool) {
            let media_res = MediaLoaded::Thumbnail { key: key.clone() };

            if self.cache.get(&key).is_some() {
                tracing::warn!("Thumbnail already cached: {key:?}");
                return (media_res, true);
            }

            self.cache.insert(key.clone(), MediaState::Loading);

            let request = MediaRequestParameters {
                source: source.clone(),
                format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(
                    UInt::new_saturating(key.1),
                    UInt::new_saturating(key.2),
                )),
            };

            let mut success = true;
            let res = match self.client.media().get_media_content(&request, true).await {
                Ok(bytes) => MediaState::loaded(iced::widget::image::Handle::from_bytes(bytes)),
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    success = false;
                    MediaState::Failed
                }
            };

            self.cache.insert(key, res);

            (media_res, success)
        }
    }

    pub type VideoCache = MediaCache<String, (Arc<Video>, Arc<matrix_sdk::media::MediaFileHandle>)>;

    impl CacheLoadingExt<MediaSource> for VideoCache {
        async fn load_content(&self, source: &MediaSource) -> (MediaLoaded, bool) {
            use matrix_sdk::media::UniqueKey;

            let key = &source.unique_key();
            let media_res = MediaLoaded::Video { key: key.clone() };

            if self.cache.get(key).is_some() {
                return (media_res, true);
            }

            self.cache.insert(key.clone(), MediaState::Loading);

            let request = &MediaRequestParameters {
                source: source.clone(),
                format: MediaFormat::File,
            };

            let mut success = true;
            let res = match self
                .client
                .media()
                .get_media_file(request, None, &mime::TEXT_PLAIN, true, None)
                .await
            {
                Ok(file) => {
                    let url = match url::Url::from_file_path(file.path()) {
                        Ok(url) => url,
                        Err(_) => {
                            tracing::error!("Failed to parse video file path");
                            return (media_res, false);
                        }
                    };

                    match iced_video_player::Video::new(&url) {
                        Ok(video) => {
                            video.set_paused(true);
                            video.set_looping(true);
                            MediaState::Loaded(Arc::new((Arc::new(video), Arc::new(file))))
                        }
                        Err(e) => {
                            tracing::error!("Failed to play video file: {e}");
                            success = false;
                            MediaState::Failed
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    success = false;
                    MediaState::Failed
                }
            };

            self.cache.insert(key.clone(), res);

            (media_res, success)
        }
    }

    pub type ImageCache = MediaCache<String, ImageHandle>;

    impl CacheLoadingExt<MediaSource> for ImageCache {
        async fn load_content(&self, source: &MediaSource) -> (MediaLoaded, bool) {
            use matrix_sdk::media::UniqueKey;

            let key = &source.unique_key();
            let media_res = MediaLoaded::Video { key: key.clone() };

            if self.cache.get(key).is_some() {
                return (media_res, true);
            }

            self.cache.insert(key.clone(), MediaState::Loading);

            let request = &MediaRequestParameters {
                source: source.clone(),
                format: MediaFormat::File,
            };

            let mut success = true;
            let res = match self.client.media().get_media_content(request, true).await {
                Ok(bytes) => MediaState::Loaded(Arc::new(ImageHandle::from_bytes(bytes))),
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    success = false;
                    MediaState::Failed
                }
            };

            self.cache.insert(key.clone(), res);

            (media_res, success)
        }
    }
}
