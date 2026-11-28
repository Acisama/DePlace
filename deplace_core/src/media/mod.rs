use std::{
    hash::Hash,
    sync::{Arc, atomic::Ordering},
};

use cache::{Image, ImageCache, ThumbnailLoader};
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings, UniqueKey},
};
use ruma::{OwnedMxcUri, UInt, events::room::MediaSource, uint};

use crate::{Shared, formatting::format_avatar_name};

#[cfg(feature = "iced")]
pub mod video;

pub mod cache;

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
    ) -> impl Future<Output = MediaState<Self::Item>> + Send;
}

#[derive(Clone)]
pub enum MediaState<T: Clone> {
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
    },
    Loading,
    Failed(Arc<String>),
    Loaded {
        data: T,
        size: u64,
        dimensions: Option<(u32, u32)>,
    },
}

impl<T: Clone> MediaState<T> {
    fn size(&self) -> Option<u64> {
        match self {
            MediaState::Loaded { size, .. } => Some(*size),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct DePlaceMedia<T: Clone, Extra: Clone> {
    inner: Arc<DePlaceMediaInner<T, Extra>>,
}

pub struct DePlaceMediaInner<T: Clone, Extra> {
    /// The source of the media, can be used to fetch the original file.
    source: MediaSource,
    /// The relevent parmeters for the media (e.g. an image would have a thumbnail here).
    parameters: MediaRequestParameters,
    parameters_key: String,
    state: Shared<MediaState<T>>,
    extra: Extra,

    size: Option<u64>,
    dimensions: (u32, u32),

    name: String,
}

impl<T: Clone, E: Clone> std::fmt::Debug for DePlaceMedia<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.inner.state.read().as_ref() {
            MediaState::LoadNotStarted { .. } => write!(f, "LoadNotStarted"),
            MediaState::Loading => write!(f, "Loading"),
            MediaState::Failed(_) => write!(f, "Failed"),
            MediaState::Loaded { .. } => write!(f, "Loaded"),
        }
    }
}

impl<T: Clone> Hash for MediaState<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            MediaState::LoadNotStarted { .. } => 0.hash(state),
            MediaState::Loading => 1.hash(state),
            MediaState::Failed(_) => 2.hash(state),
            MediaState::Loaded { .. } => 3.hash(state),
        }
    }
}

impl<T: Clone> PartialEq for MediaState<T> {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (
                MediaState::LoadNotStarted { .. },
                MediaState::LoadNotStarted { .. }
            ) | (MediaState::Loading, MediaState::Loading)
                | (MediaState::Failed(_), MediaState::Failed(_))
                | (MediaState::Loaded { .. }, MediaState::Loaded { .. })
        )
    }
}

impl<I: Clone + Sync + Send + 'static> Shared<MediaState<I>> {
    pub fn has_started(&self) -> bool {
        !matches!(**self.read(), MediaState::LoadNotStarted { .. })
    }

    fn media_stuff(&self) -> Option<(Client, MediaRequestParameters, tokio::runtime::Handle)> {
        let mut stuff = None;

        self.data.rcu(|current| match current.as_ref() {
            MediaState::LoadNotStarted {
                client,
                parameters,
                handle,
            } => {
                stuff = Some((client.clone(), parameters.clone(), handle.clone()));
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
}

impl<I: Clone + Sync + Send + 'static, Extra: Clone + Send + Sync + 'static>
    DePlaceMedia<I, Extra>
{
    fn new(
        source: MediaSource,
        parameters: MediaRequestParameters,
        state: Shared<MediaState<I>>,
        extra: Extra,
        size: Option<u64>,
        dimensions: (u32, u32),
        name: String,
    ) -> Self {
        Self {
            inner: Arc::new(DePlaceMediaInner {
                source,
                parameters_key: parameters.unique_key(),
                parameters,
                state,
                extra,
                size,
                dimensions,
                name,
            }),
        }
    }

    pub fn filename(&self) -> &str {
        &self.inner.name
    }

    pub fn source(&self) -> &MediaSource {
        &self.inner.source
    }

    pub fn parameters(&self) -> &MediaRequestParameters {
        &self.inner.parameters
    }

    pub fn state(&self) -> &Shared<MediaState<I>> {
        &self.inner.state
    }

    pub fn size(&self) -> Option<u64> {
        self.inner.size
    }

    pub fn parameters_key(&self) -> &str {
        &self.inner.parameters_key
    }

    pub fn actual_size(&self) -> Option<u64> {
        if let Some(size) = self.state().read().size() {
            Some(size)
        } else {
            self.size()
        }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.inner.dimensions
    }

    pub fn extra(&self) -> &Extra {
        &self.inner.extra
    }

    pub fn start_loading<T: MediaLoader<Extra, Item = I>>(&self, fallback: bool) {
        let Some((client, parameters, handle)) = self.inner.state.media_stuff() else {
            return;
        };

        let extra = self.inner.extra.clone();
        let inner = self.inner.clone();

        handle.spawn(async move {
            let result = T::fetch(client, parameters, extra, fallback).await;
            inner
                .state
                .change_if_modified(|state| *state = result.clone());
        });
    }

    pub fn as_ptr(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }
}

#[derive(Clone)]
pub struct ProfileAvatars {
    small: Image,
    medium: Image,
    large: Image,
    extra_large: Image,

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
    pub(crate) fn get(cache: &ImageCache, avatar_url: OwnedMxcUri, id: &str) -> Self {
        let small = (25, uint!(25));
        let medium = (50, uint!(50));
        let large = (100, uint!(100));
        let extra_large = (200, uint!(200));

        let image = |(size, usize)| {
            cache.get(
                &MediaRequestParameters {
                    source: ruma::events::room::MediaSource::Plain(avatar_url.clone()),
                    format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(usize, usize)),
                },
                None,
                (size, size),
                &format_avatar_name(id, size),
            )
        };

        Self {
            small: image(small),
            medium: image(medium),
            large: image(large),
            extra_large: image(extra_large),

            small_thumbnail_size: small.1,
            medium_thumbnail_size: medium.1,
            large_thumbnail_size: large.1,
            extra_large_thumbnail_size: extra_large.1,
        }
    }

    pub fn best_fitting(&self, size: f64) -> Image {
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
