//! Loads videos through matrix-sdk's media store: the file is kept in the
//! encrypted sqlite db and written to a temp file that gstreamer plays.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use iced_video_player::Video as IcedVideo;
use matrix_sdk::{
    Client,
    media::{
        MediaFileHandle, MediaFormat, MediaRequestParameters, UniqueKey,
        store::IgnoreMediaRetentionPolicy,
    },
};
use sha2::{Digest, Sha256};

use crate::state::media_fetcher::MediaProcessor;

use super::{DePlaceMedia, MediaLoader, MediaState, cache::MediaCache};

pub type Video = DePlaceMedia<LoadedVideo, VideoCacheExtra>;

/// A loaded video and the temp file it plays from, which is removed once both
/// are dropped.
pub type LoadedVideo = (Arc<IcedVideo>, Arc<MediaFileHandle>);

/// The video cache's surroundings: the media store holds the files, the marker
/// directory only remembers which of them are in it.
#[derive(Clone, Debug)]
pub struct VideoCacheExtra {
    pub media_processor: MediaProcessor,
    marker_dir: Arc<PathBuf>,
}

impl VideoCacheExtra {
    pub fn new(cache_dir: &Path, media_processor: MediaProcessor) -> Self {
        // Older versions kept the video files themselves in the cache dir, the
        // media store holds them now, so reclaim the space in the background.
        let obsolete: Vec<PathBuf> = ["videos", "videos-v2", "encrypted_videos", "videos-loaded"]
            .into_iter()
            .map(|dir| cache_dir.join(dir))
            .filter(|dir| dir.exists())
            .collect();
        if !obsolete.is_empty() {
            tokio::task::spawn_blocking(move || {
                for dir in obsolete {
                    if let Err(e) = std::fs::remove_dir_all(&dir) {
                        tracing::warn!("Failed to remove the obsolete video cache dir: {e}");
                    }
                }
            });
        }

        // Versioned so the previous scheme's markers do not claim a video is
        // already cached: everything downloads once with the new flow.
        let marker_dir = cache_dir.join("videos-loaded-v2");
        if let Err(e) = std::fs::create_dir_all(&marker_dir) {
            tracing::warn!("Failed to create the video marker dir: {e}");
        }

        Self {
            media_processor,
            marker_dir: Arc::new(marker_dir),
        }
    }

    /// Whether this video was already loaded, i.e. its file is in the media
    /// store: loading it is a local read rather than a download.
    pub fn is_cached(&self, key: &str) -> bool {
        self.marker_path(key).is_file()
    }

    /// Reading the store to answer this is async, so a load that succeeded
    /// leaves an empty marker behind for the next run to find.
    fn marker_path(&self, key: &str) -> PathBuf {
        let hex: String = Sha256::digest(key.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.marker_dir.join(hex)
    }
}

impl MediaLoader<VideoCacheExtra> for LoadedVideo {
    type Item = LoadedVideo;

    async fn fetch(
        client: Client,
        parameters: MediaRequestParameters,
        extra: VideoCacheExtra,
        _fallback: bool,
    ) -> MediaState<Self::Item> {
        let key = parameters.unique_key();
        let cached = extra.is_cached(&key);

        match load(client, parameters, &extra.media_processor, cached).await {
            Ok((loaded, size)) => {
                if let Err(e) = std::fs::write(extra.marker_path(&key), []) {
                    tracing::warn!("Failed to write the video marker: {e}");
                }
                MediaState::Loaded {
                    data: loaded,
                    size,
                    dimensions: None,
                }
            }
            Err(e) => {
                // A video that no longer loads must ask again next time.
                if let Err(e) = std::fs::remove_file(extra.marker_path(&key))
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!("Failed to remove the video marker: {e}");
                }
                MediaState::Failed(Arc::new(e))
            }
        }
    }
}

async fn load(
    client: Client,
    parameters: MediaRequestParameters,
    media_processor: &MediaProcessor,
    cached: bool,
) -> Result<(LoadedVideo, u64), String> {
    let request = MediaRequestParameters {
        source: parameters.source,
        format: MediaFormat::File,
    };

    // A video that was played before is already in the media store, any other
    // one is downloaded now, reporting progress the UI shows meanwhile.
    if !cached {
        let bytes = media_processor.download(&client, &request).await?;
        if let Err(e) = store_bytes(&client, &request, bytes).await {
            // Playing it matters more than caching it.
            tracing::warn!("Failed to store the downloaded video: {e}");
        }
    }

    let file = client
        .media()
        .get_media_file(&request, None, &mime::APPLICATION_OCTET_STREAM, true, None)
        .await
        .map_err(|e| e.to_string())?;

    let path = file.path();

    let size = tokio::fs::metadata(path)
        .await
        .map_err(|e| e.to_string())?
        .len();

    let url = url::Url::from_file_path(file.path())
        .map_err(|()| format!("invalid video temp file path: {}", path.display()))?;

    let video = IcedVideo::new(&url).map_err(|e| e.to_string())?;
    video.set_paused(true);

    Ok(((Arc::new(video), Arc::new(file)), size))
}

/// Keeps a downloaded video, so playing it again is a local read.
async fn store_bytes(
    client: &Client,
    request: &MediaRequestParameters,
    bytes: Vec<u8>,
) -> Result<(), String> {
    let store = client
        .media_store()
        .lock()
        .await
        .map_err(|e| e.to_string())?;

    store
        .add_media_content(request, bytes, IgnoreMediaRetentionPolicy::No)
        .await
        .map_err(|e| e.to_string())
}

/// A video cache, backed by the SDK's media store.
pub type VideoCache = MediaCache<LoadedVideo, VideoCacheExtra>;
