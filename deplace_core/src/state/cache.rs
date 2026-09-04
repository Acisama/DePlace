#[cfg(feature = "iced_desktop")]
use std::{hash::Hash, sync::Arc};

use dashmap::DashMap;
#[cfg(feature = "iced_desktop")]
use iced_video_player::Video;
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
};
use ruma::{OwnedMxcUri, UInt};
use ruma::{events::room::MediaSource, uint};

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

#[cfg(feature = "iced_desktop")]
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
        filename: String,
    },
}

#[cfg(feature = "iced_desktop")]
impl NeedsMedia {
    pub fn avatar(uri: OwnedMxcUri) -> Self {
        Self::Avatar { uri }
    }

    pub fn thumbnail(source: MediaSource, key: (String, u64, u64)) -> Self {
        Self::Thumbnail { source, key }
    }

    pub fn video(source: MediaSource, filename: String) -> Self {
        Self::Video { source, filename }
    }
}

#[cfg(feature = "iced_desktop")]
#[derive(Clone, Debug)]
pub enum MediaLoaded {
    Thumbnail { key: (String, u64, u64) },
    Avatar { uri: OwnedMxcUri },
    Video { key: String },
}

#[cfg(feature = "iced_desktop")]
pub type AvatarCache = MediaCache<OwnedMxcUri, iced::widget::image::Handle>;

#[cfg(feature = "iced_desktop")]
impl AvatarCache {
    /// Loads an avatar with the given URI and returns MediaLoaded and a boolean indicating whether it was successfully loaded.
    pub async fn load_avatar(&self, uri: OwnedMxcUri) -> (MediaLoaded, bool) {
        let media_res = MediaLoaded::Avatar { uri: uri.clone() };

        if self.cache.get(&uri).is_some() {
            return (media_res, true);
        }

        self.cache.insert(uri.clone(), MediaState::Loading);

        let request = MediaRequestParameters {
            source: MediaSource::Plain(uri.clone()),
            format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(uint!(100), uint!(100))),
        };

        let mut success = true;
        let res = match self.client.media().get_media_content(&request, false).await {
            Ok(bytes) => MediaState::loaded(iced::widget::image::Handle::from_bytes(bytes)),
            Err(e) => {
                tracing::error!("Failed to fetch media: {e}");
                success = false;
                MediaState::Failed
            }
        };

        self.cache.insert(uri.clone(), res);

        return (media_res, success);
    }
}

#[cfg(feature = "iced_desktop")]
pub type ThumbnailCache = MediaCache<(String, u64, u64), iced::widget::image::Handle>;

#[cfg(feature = "iced_desktop")]
impl ThumbnailCache {
    /// Loads a thumbnail with the given key (incldues it's size) and returns MediaLoaded and a boolean indicating whether it was successfully loaded.
    pub async fn load_thumbnail(
        &self,
        source: MediaSource,
        key: (String, u64, u64),
    ) -> (MediaLoaded, bool) {
        let media_res = MediaLoaded::Thumbnail { key: key.clone() };

        if self.cache.get(&key).is_some() {
            tracing::warn!("Thumbnail already cached: {key:?}");
            return (media_res, true);
        }

        self.cache.insert(key.clone(), MediaState::Loading);

        let request = MediaRequestParameters {
            source,
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

        return (media_res, success);
    }
}

#[cfg(feature = "iced_desktop")]
pub type VideoCache = MediaCache<String, (Arc<Video>, Arc<matrix_sdk::media::MediaFileHandle>)>;

#[cfg(feature = "iced_desktop")]
impl VideoCache {
    /// Loads a video with the given source and filename and returns MediaLoaded and a boolean indicating whether it was successfully loaded.
    pub async fn load_video(&self, source: MediaSource, filename: String) -> (MediaLoaded, bool) {
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
            .get_media_file(
                request,
                Some(filename.clone()),
                &mime::TEXT_PLAIN,
                true,
                None,
            )
            .await
        {
            Ok(file) => {
                let url = url::Url::from_file_path(file.path()).unwrap();

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

        return (media_res, success);
    }
}
