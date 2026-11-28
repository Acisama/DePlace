use std::any::type_name;

use crate::shared::Shared;
use dashmap::DashMap;
pub use iced_caches::*;
use matrix_sdk::{
    Client,
    media::{MediaRequestParameters, UniqueKey},
};

use super::{DePlaceMedia, MediaLoader, MediaState};

pub struct MediaCache<T: Clone + MediaLoader<Extra>, Extra: Clone> {
    client: Client,
    handle: tokio::runtime::Handle,
    #[allow(clippy::type_complexity)]
    cache: DashMap<String, DePlaceMedia<T::Item, Extra>>,
    extra: Extra,
}

impl<T: MediaLoader<Extra> + Clone, Extra: Clone> std::fmt::Debug for MediaCache<T, Extra> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("MediaCache<{}>", type_name::<T>()))
    }
}

impl<T: MediaLoader<Extra> + Clone, Extra: Clone + Send + Sync + 'static> MediaCache<T, Extra> {
    pub(crate) fn new(client: Client, extra: Extra) -> Self {
        MediaCache {
            client,
            handle: tokio::runtime::Handle::current(),
            cache: DashMap::new(),
            extra,
        }
    }

    pub fn extra(&self) -> &Extra {
        &self.extra
    }

    /// Returns the cell for this media, creating it (as `LoadNotStarted`) if
    /// it doesn't exist yet. Never fetches anything by itself -- call
    /// [`Shared::start_loading`] on the result once it's actually rendered.
    pub fn get(
        &self,
        parameters: &MediaRequestParameters,
        size: Option<u64>,
        dimensions: (u32, u32),
        name: &str,
    ) -> DePlaceMedia<T::Item, Extra> {
        self.cache
            .entry(parameters.unique_key())
            .or_insert_with(|| {
                DePlaceMedia::new(
                    parameters.source.clone(),
                    parameters.clone(),
                    Shared::new(MediaState::LoadNotStarted {
                        client: self.client.clone(),
                        parameters: parameters.clone(),
                        handle: self.handle.clone(),
                    }),
                    self.extra.clone(),
                    size,
                    dimensions,
                    name.to_owned(),
                )
            })
            .clone()
    }
}

/// SVGs have no magic number (they're XML text, not a binary format), so
/// `image::guess_format`/`ImageReader::with_guessed_format` never recognize
/// them. Sniff the start of the payload for an `<svg` tag instead, which
/// tolerates a leading BOM or `<?xml` prolog.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    String::from_utf8_lossy(head).contains("<svg")
}

#[cfg(feature = "iced")]
mod iced_caches {
    use std::{io::Cursor, sync::Arc};

    use iced::advanced::{image::Handle as ImageHandle, svg::Handle as SvgHandle};
    use matrix_sdk::{
        Client,
        media::{MediaFormat, MediaRequestParameters},
    };

    use super::{DePlaceMedia, MediaCache, MediaLoader, MediaState, looks_like_svg};

    #[derive(Debug, Clone)]
    pub enum DePlaceImage {
        Raster(ImageHandle),
        Svg(SvgHandle),
    }

    impl DePlaceImage {
        fn from_bytes(bytes: Vec<u8>) -> Self {
            if looks_like_svg(&bytes) {
                DePlaceImage::Svg(SvgHandle::from_memory(bytes))
            } else {
                DePlaceImage::Raster(ImageHandle::from_bytes(bytes))
            }
        }
    }

    #[derive(Clone)]
    pub struct ThumbnailLoader;
    #[derive(Clone)]
    pub struct ImageLoader;

    impl MediaLoader<()> for ThumbnailLoader {
        type Item = DePlaceImage;

        async fn fetch(
            client: Client,
            parameters: MediaRequestParameters,
            _extra: (),
            fallback: bool,
        ) -> MediaState<Self::Item> {
            match client.media().get_media_content(&parameters, true).await {
                Ok(bytes) => MediaState::Loaded {
                    size: bytes.len() as u64,
                    data: DePlaceImage::from_bytes(bytes),
                    dimensions: None,
                },
                // The homeserver rejects thumbnail requests for formats it can't
                // rasterize (e.g. SVG) instead of just serving the original, so
                // fall back to fetching the full file in that case.
                Err(e) if fallback && matches!(parameters.format, MediaFormat::Thumbnail(_)) => {
                    tracing::trace!(
                        "Thumbnail request failed ({e}), falling back to the full file"
                    );
                    let file_request = MediaRequestParameters {
                        source: parameters.source.clone(),
                        format: MediaFormat::File,
                    };
                    match client.media().get_media_content(&file_request, true).await {
                        Ok(bytes) => MediaState::Loaded {
                            size: bytes.len() as u64,
                            data: DePlaceImage::from_bytes(bytes),
                            dimensions: None,
                        },
                        Err(e) => {
                            tracing::error!("Failed to fetch media: {e}");
                            MediaState::Failed(Arc::new(e.to_string()))
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to fetch media: {e}");
                    MediaState::Failed(Arc::new(e.to_string()))
                }
            }
        }
    }

    impl MediaLoader<()> for ImageLoader {
        type Item = DePlaceImage;

        async fn fetch(
            client: Client,
            parameters: MediaRequestParameters,
            _extra: (),
            _fallback: bool,
        ) -> MediaState<Self::Item> {
            match client.media().get_media_content(&parameters, true).await {
                Ok(bytes) => {
                    if looks_like_svg(&bytes) {
                        MediaState::Loaded {
                            size: bytes.len() as u64,
                            data: DePlaceImage::Svg(SvgHandle::from_memory(bytes)),
                            dimensions: None,
                        }
                    } else {
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
                        MediaState::Loaded {
                            size: bytes.len() as u64,
                            data: DePlaceImage::Raster(ImageHandle::from_bytes(bytes)),
                            dimensions,
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    MediaState::Failed(Arc::new(e.to_string()))
                }
            }
        }
    }

    pub type ImageCache = MediaCache<ImageLoader, ()>;
    pub type Image = DePlaceMedia<DePlaceImage, ()>;
}

#[cfg(not(feature = "iced"))]
mod iced_caches {
    pub type ThumbnailCache = MediaCache<()>;
    pub type ImageCache = MediaCache<()>;
    pub type Thumbnail = DePlaceMedia<(), ()>;
    pub type Image = DePlaceMedia<(), ()>;
}
