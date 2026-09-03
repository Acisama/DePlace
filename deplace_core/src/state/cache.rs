use std::{hash::Hash, sync::Arc};

use dashmap::DashMap;
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
pub type AvatarCache = MediaCache<OwnedMxcUri, iced::widget::image::Handle>;

#[cfg(feature = "iced_desktop")]
impl AvatarCache {
    pub async fn load_avatar(&self, uri: &OwnedMxcUri) {
        if self.cache.get(uri).is_some() {
            return;
        }

        self.cache.insert(uri.clone(), MediaState::Loading);

        let request = MediaRequestParameters {
            source: MediaSource::Plain(uri.clone()),
            format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(uint!(100), uint!(100))),
        };

        let res = match self.client.media().get_media_content(&request, true).await {
            Ok(bytes) => MediaState::loaded(iced::widget::image::Handle::from_bytes(bytes)),
            Err(e) => {
                tracing::error!("Failed to fetch media: {e}");
                MediaState::Failed
            }
        };

        self.cache.insert(uri.clone(), res);
    }
}

#[cfg(feature = "iced_desktop")]
pub type ThumbnailCache = MediaCache<(String, u64, u64), iced::widget::image::Handle>;

#[cfg(feature = "iced_desktop")]
impl ThumbnailCache {
    pub async fn load_thumbnail(&self, source: MediaSource, key: (String, u64, u64)) {
        if self.cache.get(&key).is_some() {
            return;
        }

        self.cache.insert(key.clone(), MediaState::Loading);

        let request = MediaRequestParameters {
            source,
            format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(
                UInt::new_saturating(key.1),
                UInt::new_saturating(key.2),
            )),
        };

        let res = match self.client.media().get_media_content(&request, true).await {
            Ok(bytes) => MediaState::loaded(iced::widget::image::Handle::from_bytes(bytes)),
            Err(e) => {
                tracing::error!("Failed to fetch media: {e}");
                MediaState::Failed
            }
        };

        self.cache.insert(key, res);
    }
}
