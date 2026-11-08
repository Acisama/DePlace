use std::{pin::Pin, sync::Arc};

use dashmap::DashMap;
use matrix_sdk::{
    Client,
    media::{MediaRequestParameters, UniqueKey},
};

use crate::Shared;

#[derive(Default, Debug, Clone)]
pub enum MediaState<T: Clone> {
    #[default]
    Loading,
    Failed(Arc<String>),
    Loaded(T),
}

pub type FetchingFn<T> = Arc<
    dyn Fn(Client, MediaRequestParameters) -> Pin<Box<dyn Future<Output = MediaState<T>> + Send>>
        + Sync
        + Send,
>;

pub struct MediaCache<T: Clone + MediaLoader> {
    client: Client,
    pub cache: DashMap<String, Arc<Shared<MediaState<T::Item>>>>,
}

pub trait MediaLoader: Send + Sync + 'static {
    type Item: Clone + Sync + Send + 'static;
    fn fetch(
        client: Client,
        parameters: MediaRequestParameters,
    ) -> impl Future<Output = MediaState<Self::Item>> + Send;
}

impl<T: MediaLoader + Clone> MediaCache<T> {
    pub(crate) fn new(client: Client) -> Self {
        MediaCache {
            client,
            cache: DashMap::new(),
        }
    }

    pub fn get(&self, parmeters: &MediaRequestParameters) -> Arc<Shared<MediaState<T::Item>>> {
        let mut created_new = false;
        let entry = self.cache.entry(parmeters.unique_key()).or_insert_with(|| {
            created_new = true;
            Arc::new(Shared::new(MediaState::Loading))
        });
        let cell = entry.value().clone();

        if created_new {
            self.spawn_load(parmeters.clone(), cell.clone());
        }

        cell.clone()
    }

    fn spawn_load(
        &self,
        parameters: MediaRequestParameters,
        cell: Arc<Shared<MediaState<T::Item>>>,
    ) {
        let client = self.client.clone();

        tokio::spawn(async move {
            let result = T::fetch(client, parameters).await;

            cell.change(move |c| {
                *c = result.clone();
            });
        });
    }
}

#[cfg(feature = "iced")]
mod iced_caches {
    use std::{io::Cursor, sync::Arc};

    use iced::advanced::image::Handle as ImageHandle;
    use matrix_sdk::{Client, media::MediaRequestParameters};

    use super::{MediaCache, MediaLoader, MediaState};
    pub struct ThumbnailLoader;
    pub struct ImageLoader;

    impl MediaLoader for ThumbnailLoader {
        type Item = ImageHandle;

        async fn fetch(
            client: Client,
            parameters: MediaRequestParameters,
        ) -> MediaState<Self::Item> {
            match client.media().get_media_content(&parameters, true).await {
                Ok(bytes) => MediaState::Loaded(ImageHandle::from_bytes(bytes.clone())),
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    MediaState::Failed(Arc::new(e.to_string()))
                }
            }
        }
    }

    impl MediaLoader for ImageLoader {
        type Item = (ImageHandle, Option<(u32, u32)>);

        async fn fetch(
            client: Client,
            parameters: MediaRequestParameters,
        ) -> MediaState<Self::Item> {
            match client.media().get_media_content(&parameters, true).await {
                Ok(bytes) => {
                    let dimensions = image::ImageReader::new(Cursor::new(&bytes))
                        .with_guessed_format()
                        .map_err(|e| tracing::error!("Failed to read image: {e}"))
                        .ok()
                        .and_then(|reader| {
                            reader
                                .into_dimensions()
                                .map_err(|e| tracing::error!("Failed to read dimensions: {e}"))
                                .ok()
                        });
                    MediaState::Loaded((ImageHandle::from_bytes(bytes.clone()), dimensions))
                }
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    MediaState::Failed(Arc::new(e.to_string()))
                }
            }
        }
    }

    pub type ThumbnailCache = MediaCache<ThumbnailLoader>;
    pub type ImageCache = MediaCache<ImageLoader>;
}

#[cfg(feature = "iced")]
pub use iced_caches::{ImageCache, ThumbnailCache};
