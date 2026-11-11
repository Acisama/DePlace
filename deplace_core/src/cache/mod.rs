use std::{
    any::type_name,
    hash::Hash,
    sync::{Arc, atomic::Ordering},
};

use dashmap::DashMap;
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings, UniqueKey},
};
use ruma::{OwnedMxcUri, UInt, uint};

use crate::Shared;

#[cfg(feature = "iced")]
pub mod video;
#[cfg(feature = "iced")]
pub mod video_stream;

/// Loads one kind of media (e.g. a thumbnail vs. the original file) for a
/// [`MediaCache`]. `fetch` is only ever invoked once per cache key, by
/// whichever caller wins the race in [`Shared::start_loading`].
pub trait MediaLoader<Extra: Clone>: Send + Sync + 'static {
    type Item: Clone + Sync + Send + 'static;

    fn fetch(
        client: Client,
        parameters: MediaRequestParameters,
        extra: Extra,
        fallback: bool,
    ) -> impl Future<Output = MediaState<Self::Item, Extra>> + Send;
}

#[derive(Clone)]
pub enum MediaState<T: Clone, Extra: Clone> {
    /// Contains everything needed to fetch the media. `handle` is carried
    /// along rather than relying on an ambient Tokio context at
    /// `start_loading` time: that gets called from widget code during
    /// `view()`, which isn't guaranteed to run inside a Tokio-entered
    /// thread the way `update()` is, so `tokio::spawn` there can panic
    /// with "there is no reactor running". Capturing the handle once, back
    /// when the cache itself was constructed (`MediaCache::new`, always
    /// called from an async context), and spawning through it instead
    /// sidesteps that entirely.
    LoadNotStarted {
        client: Client,
        parameters: MediaRequestParameters,
        handle: tokio::runtime::Handle,
        extra: Extra,
    },
    Loading,
    Failed(Arc<String>),
    Loaded(T),
}

impl<T: Clone, Extra: Clone> std::fmt::Debug for MediaState<T, Extra> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaState::LoadNotStarted { .. } => write!(f, "LoadNotStarted"),
            MediaState::Loading => write!(f, "Loading"),
            MediaState::Failed(_) => write!(f, "Failed"),
            MediaState::Loaded(_) => write!(f, "Loaded"),
        }
    }
}

impl<T: Clone, Extra: Clone> Hash for MediaState<T, Extra> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            MediaState::LoadNotStarted { .. } => 0.hash(state),
            MediaState::Loading => 1.hash(state),
            MediaState::Failed(_) => 2.hash(state),
            MediaState::Loaded(_) => 3.hash(state),
        }
    }
}

impl<T: Clone, Extra: Clone> PartialEq for MediaState<T, Extra> {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (
                MediaState::LoadNotStarted { .. },
                MediaState::LoadNotStarted { .. }
            ) | (MediaState::Loading, MediaState::Loading)
                | (MediaState::Failed(_), MediaState::Failed(_))
                | (MediaState::Loaded(_), MediaState::Loaded(_))
        )
    }
}

impl<I: Clone + Sync + Send + 'static, Extra: Clone + Send + Sync + 'static>
    Shared<MediaState<I, Extra>>
{
    pub fn has_started(&self) -> bool {
        !matches!(**self.read(), MediaState::LoadNotStarted { .. })
    }

    /// Atomically claims the fetch for this cell: if it's currently
    /// `LoadNotStarted`, moves it to `Loading` and returns the `(client,
    /// parameters)` needed to actually fetch it; otherwise (already
    /// loading, loaded, or failed) returns `None` and leaves it untouched.
    ///
    /// The read-and-maybe-write has to happen as one `rcu` operation rather
    /// than a separate `read()` then `change()` -- otherwise two callers for
    /// the same media (e.g. a message and its reply preview, rendered at
    /// the same time) could both observe `LoadNotStarted` in the gap before
    /// either had written `Loading`, and both go on to fetch it.
    fn media_stuff(
        &self,
    ) -> Option<(
        Client,
        MediaRequestParameters,
        tokio::runtime::Handle,
        Extra,
    )> {
        let mut stuff = None;

        self.data.rcu(|current| match current.as_ref() {
            MediaState::LoadNotStarted {
                client,
                parameters,
                handle,
                extra,
            } => {
                stuff = Some((
                    client.clone(),
                    parameters.clone(),
                    handle.clone(),
                    extra.clone(),
                ));
                Arc::new(MediaState::Loading)
            }
            _ => {
                stuff = None;
                Arc::clone(current)
            }
        });

        if stuff.is_some() {
            self.version.fetch_add(1, Ordering::Relaxed);
        }

        stuff
    }

    pub fn start_loading<T: MediaLoader<Extra, Item = I>>(self: Arc<Self>, fallback: bool) {
        let Some((client, parameters, handle, extra)) = self.media_stuff() else {
            return;
        };

        handle.spawn(async move {
            let result = T::fetch(client, parameters, extra, fallback).await;
            self.change_if_modified(|state| *state = result.clone());
        });
    }
}

pub struct MediaCache<T: Clone + MediaLoader<Extra>, Extra: Clone> {
    client: Client,
    handle: tokio::runtime::Handle,
    #[allow(clippy::type_complexity)]
    cache: DashMap<String, Arc<Shared<MediaState<T::Item, Extra>>>>,
    extra: Extra,
}

impl<T: MediaLoader<Extra> + Clone, Extra: Clone> std::fmt::Debug for MediaCache<T, Extra> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("MediaCache<{}>", type_name::<T>()))
    }
}

impl<T: MediaLoader<Extra> + Clone, Extra: Clone> MediaCache<T, Extra> {
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
    ) -> Arc<Shared<MediaState<T::Item, Extra>>> {
        self.cache
            .entry(parameters.unique_key())
            .or_insert_with(|| {
                Arc::new(Shared::new(MediaState::LoadNotStarted {
                    client: self.client.clone(),
                    parameters: parameters.clone(),
                    handle: self.handle.clone(),
                    extra: self.extra.clone(),
                }))
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

    use crate::Shared;

    use super::{MediaCache, MediaLoader, MediaState, looks_like_svg};

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
        ) -> MediaState<Self::Item, ()> {
            match client.media().get_media_content(&parameters, true).await {
                Ok(bytes) => MediaState::Loaded(DePlaceImage::from_bytes(bytes)),
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
                        Ok(bytes) => MediaState::Loaded(DePlaceImage::from_bytes(bytes)),
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
        type Item = (DePlaceImage, Option<(u32, u32)>);

        async fn fetch(
            client: Client,
            parameters: MediaRequestParameters,
            _extra: (),
            _fallback: bool,
        ) -> MediaState<Self::Item, ()> {
            match client.media().get_media_content(&parameters, true).await {
                Ok(bytes) => {
                    if looks_like_svg(&bytes) {
                        MediaState::Loaded((DePlaceImage::Svg(SvgHandle::from_memory(bytes)), None))
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
                        MediaState::Loaded((
                            DePlaceImage::Raster(ImageHandle::from_bytes(bytes)),
                            dimensions,
                        ))
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    MediaState::Failed(Arc::new(e.to_string()))
                }
            }
        }
    }

    pub type ThumbnailCache = MediaCache<ThumbnailLoader, ()>;
    pub type ImageCache = MediaCache<ImageLoader, ()>;

    pub type DePlaceThumbnailShared = Arc<Shared<MediaState<DePlaceImage, ()>>>;
    pub type DePlaceImageShared = Arc<Shared<MediaState<(DePlaceImage, Option<(u32, u32)>), ()>>>;
}

#[cfg(not(feature = "iced"))]
mod iced_caches {
    pub type ThumbnailCache = MediaCache<()>;
    pub type ImageCache = MediaCache<()>;
    pub type DePlaceThumbnail = Arc<Shared<MediaState<()>>>;
    pub type DePlaceImage = Arc<Shared<MediaState<((), Option<(u32, u32)>)>>>;
}

pub use iced_caches::*;

#[derive(Clone)]
pub struct ProfileAvatars {
    small: DePlaceThumbnailShared,
    medium: DePlaceThumbnailShared,
    large: DePlaceThumbnailShared,
    extra_large: DePlaceThumbnailShared,

    small_thumbnail_size: UInt,
    medium_thumbnail_size: UInt,
    large_thumbnail_size: UInt,
    extra_large_thumbnail_size: UInt,
}

impl std::fmt::Debug for ProfileAvatars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileAvatars")
            .field("small", &self.small)
            .field("medium", &self.medium)
            .field("large", &self.large)
            .finish()
    }
}

impl ProfileAvatars {
    pub(crate) fn get(cache: &ThumbnailCache, avatar_url: OwnedMxcUri) -> Self {
        let small = uint!(25);
        let medium = uint!(50);
        let large = uint!(100);
        let extra_large = uint!(200);

        let thumbnail = |size| {
            cache.get(&MediaRequestParameters {
                source: ruma::events::room::MediaSource::Plain(avatar_url.clone()),
                format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(size, size)),
            })
        };

        Self {
            small: thumbnail(small),
            medium: thumbnail(medium),
            large: thumbnail(large),
            extra_large: thumbnail(extra_large),

            small_thumbnail_size: small,
            medium_thumbnail_size: medium,
            large_thumbnail_size: large,
            extra_large_thumbnail_size: extra_large,
        }
    }

    pub fn best_fitting(&self, size: f64) -> DePlaceThumbnailShared {
        let scaling_factor = 1.0;
        let physical_size = size * scaling_factor;
        let physical_area = physical_size * physical_size;

        let small_area: f64 = self.small_thumbnail_size.saturating_pow(2).into();
        let medium_area: f64 = self.medium_thumbnail_size.saturating_pow(2).into();
        let large_area: f64 = self.large_thumbnail_size.saturating_pow(2).into();
        let extra_large_area: f64 = self.extra_large_thumbnail_size.saturating_pow(2).into();

        if physical_area <= small_area {
            self.small.clone()
        } else if physical_area <= medium_area {
            self.medium.clone()
        } else if physical_area <= large_area {
            self.large.clone()
        } else if physical_area <= extra_large_area {
            self.extra_large.clone()
        } else {
            // If it exceeds extra large, fall back to the largest available
            self.extra_large.clone()
        }
    }

    pub fn start_load_all(&self) {
        self.small.clone().start_loading::<ThumbnailLoader>(true);
        self.medium.clone().start_loading::<ThumbnailLoader>(true);
        self.large.clone().start_loading::<ThumbnailLoader>(true);
    }
}
