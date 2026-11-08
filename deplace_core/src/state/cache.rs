use std::{hash::Hash, sync::Arc};

use dashmap::DashMap;
use matrix_sdk::Client;
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

    pub fn as_option(&self) -> Option<Arc<T>> {
        match self {
            MediaState::Loading | MediaState::Failed => None,
            MediaState::Loaded(arc) => Some(arc.clone()),
        }
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

#[derive(Debug, Clone)]
pub enum CacheResultStatus {
    Success,
    Failure(String),
    CacheHit,
}

#[derive(Debug, Clone)]
pub struct CacheLoadingResult {
    pub loaded: MediaLoaded,
    pub status: CacheResultStatus,
}

/// Extention trait for caches for loading content where the source can be used to  generate the key
pub trait CacheLoadingExt<T> {
    fn load_content(
        &self,
        source: &T,
    ) -> impl std::future::Future<Output = CacheLoadingResult> + Send;
}

/// Extention trait for caches for loading content where the key cannot be generated from the source and must be provided separately
pub trait CacheLoadingWithKeyExt<T, K> {
    fn load_content_with_key(
        &self,
        source: &T,
        key: K,
    ) -> impl std::future::Future<Output = CacheLoadingResult> + Send;
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
    Video {
        source: MediaSource,
    },
    Image {
        source: MediaSource,
    },
}

impl NeedsMedia {
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
    Video { key: String },
    Image { key: String },
}

#[cfg(feature = "iced")]
pub use iced_caches::{ImageCache, ThumbnailCache, VideoCache};

#[cfg(feature = "iced")]
mod iced_caches {
    use std::{
        io::Cursor,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use iced_video_player::Video;
    use matrix_sdk::{
        Client,
        media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    };
    use ruma::{UInt, events::room::MediaSource};

    use iced::widget::image::Handle as ImageHandle;

    use super::{
        CacheLoadingExt, CacheLoadingResult, CacheLoadingWithKeyExt, CacheResultStatus, MediaCache,
        MediaLoaded, MediaState,
    };

    pub type ThumbnailCache = MediaCache<(String, u64, u64), ImageHandle>;

    /// Decodes downloaded thumbnail bytes into an RGBA [`ImageHandle`], off
    /// the UI thread.
    async fn decode_thumbnail(bytes: Vec<u8>, width: u64, height: u64) -> Option<ImageHandle> {
        tokio::task::spawn_blocking(move || {
            let image = image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format()
                .map_err(|e| tracing::error!("Failed to guess format of thumbnail: {e}"))
                .ok()?
                .decode()
                .map_err(|e| tracing::error!("Failed to decode thumbnail: {e}"))
                .ok()?;

            let thumbnail = image.thumbnail(width as u32, height as u32).to_rgba8();

            Some(ImageHandle::from_rgba(
                thumbnail.width(),
                thumbnail.height(),
                thumbnail.into_raw(),
            ))
        })
        .await
        .ok()
        .flatten()
    }

    impl CacheLoadingWithKeyExt<MediaSource, (String, u64, u64)> for ThumbnailCache {
        /// Loads a thumbnail with the given key (incldues it's size) and returns MediaLoaded and a boolean indicating whether it was successfully loaded.
        async fn load_content_with_key(
            &self,
            source: &MediaSource,
            key: (String, u64, u64),
        ) -> CacheLoadingResult {
            let media_res = MediaLoaded::Thumbnail { key: key.clone() };

            if self.cache.get(&key).is_some() {
                tracing::warn!("Thumbnail already cached: {key:?}");
                return CacheLoadingResult {
                    loaded: media_res,
                    status: CacheResultStatus::CacheHit,
                };
            }

            self.cache.insert(key.clone(), MediaState::Loading);

            let request = MediaRequestParameters {
                source: source.clone(),
                format: MediaFormat::Thumbnail(MediaThumbnailSettings::new(
                    UInt::new_saturating(key.1),
                    UInt::new_saturating(key.2),
                )),
            };

            let mut status = CacheResultStatus::Success;
            let res = match self.client.media().get_media_content(&request, true).await {
                Ok(bytes) => match decode_thumbnail(bytes, key.1, key.2).await {
                    Some(handle) => MediaState::loaded(handle),
                    None => {
                        status =
                            CacheResultStatus::Failure("Failed to decode thumbnail".to_string());
                        MediaState::Failed
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    status = CacheResultStatus::Failure(e.to_string());
                    MediaState::Failed
                }
            };

            self.cache.insert(key, res);

            CacheLoadingResult {
                loaded: media_res,
                status,
            }
        }
    }

    /// A loaded video: the player plus the streaming feeder keeping its
    /// data source alive.
    pub type LoadedVideo = (Arc<Video>, Option<crate::state::video_stream::StreamFeeder>);

    /// The cipher used to encrypt the locally generated video key material in
    /// the sidecar files. Wrap type: [`StoreCipher`] doesn't implement Debug.
    #[derive(Clone)]
    struct SidecarCipher(Arc<matrix_sdk_store_encryption::StoreCipher>);

    /// Error of setting up the sidecar cipher.
    #[derive(Debug, thiserror::Error)]
    pub enum StoreEncryptionError {
        #[error(transparent)]
        Cipher(#[from] matrix_sdk_store_encryption::Error),
        #[error("failed to persist the cipher: {0}")]
        Io(#[from] std::io::Error),
    }

    impl std::fmt::Debug for SidecarCipher {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_tuple("SidecarCipher").finish()
        }
    }

    /// How the loaded sidecar cipher relates to what was persisted on disk.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CipherRestore {
        /// The persisted export was restored with the current store key.
        Restored,
        /// No export existed yet; a fresh cipher was created and persisted.
        Created,
        /// An export existed but could not be imported with the current store
        /// key (e.g. the keyring store key was regenerated on a fresh
        /// login). A fresh cipher was persisted; every sidecar written with
        /// the old cipher is permanently undecryptable and its cache entry
        /// must be invalidated by the caller.
        Rotated,
    }

    impl SidecarCipher {
        /// Loads the cipher of the cache directory, creating and persisting it
        /// (encrypted under the keyring store key) if it doesn't exist yet.
        ///
        /// If a persisted export exists but cannot be imported with the given
        /// store key, a fresh cipher is persisted and
        /// [`CipherRestore::Rotated`] is returned: the caller must
        /// invalidate everything that was encrypted with the old cipher, as
        /// it can never be decrypted again.
        fn new(
            cache_dir: &Path,
            store_key: &[u8],
        ) -> Result<(Self, CipherRestore), StoreEncryptionError> {
            const CIPHER_FILE: &str = ".sidecar-cipher";

            let path = cache_dir.join(CIPHER_FILE);
            let persisted = path.is_file();

            if persisted
                && let Some(cipher) = std::fs::read(&path).ok().and_then(|export| {
                    matrix_sdk_store_encryption::StoreCipher::import_with_key(store_key, &export)
                        .ok()
                })
            {
                return Ok((Self(Arc::new(cipher)), CipherRestore::Restored));
            }

            let cipher = matrix_sdk_store_encryption::StoreCipher::new()?;
            let export = cipher.export_with_key(store_key)?;
            if persisted {
                tracing::warn!(
                    "Failed to restore the persisted sidecar cipher with the \
                     current store key; creating a new one invalidates the \
                     key material of existing cached videos"
                );
            }
            std::fs::write(&path, export).map_err(StoreEncryptionError::Io)?;
            Ok((
                Self(Arc::new(cipher)),
                if persisted {
                    CipherRestore::Rotated
                } else {
                    CipherRestore::Created
                },
            ))
        }

        fn encrypt_info(
            &self,
            info: &matrix_sdk_base::crypto::MediaEncryptionInfo,
        ) -> Result<Vec<u8>, String> {
            self.0.encrypt_value(info).map_err(|e| e.to_string())
        }

        fn decrypt_info(
            &self,
            bytes: &[u8],
        ) -> Result<matrix_sdk_base::crypto::MediaEncryptionInfo, String> {
            self.0.decrypt_value(bytes).map_err(|e| e.to_string())
        }
    }

    /// Removes every cache entry whose decryption info lives in a sidecar
    /// file, i.e. every plain-source download: after a cipher rotation (see
    /// [`CipherRestore::Rotated`]) their key material can never be decrypted
    /// again, so failing on first click one by one is avoided by removing
    /// them up front.
    ///
    /// Entries of encrypted attachments have no sidecar and stay valid:
    /// their key material is part of the event itself.
    fn invalidate_stale_sidecars(cache_dir: &Path) {
        let entries = match std::fs::read_dir(cache_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to list video cache dir for invalidation: {e}");
                return;
            }
        };

        let mut removed = 0u64;
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            // The sidecar (and its partial) of a cached video: `<hex>.json`,
            // `<hex>.json.part`.
            let Some(base) = name
                .strip_suffix(".json")
                .or_else(|| name.strip_suffix(".json.part"))
                .map(str::to_owned)
            else {
                continue;
            };
            let path = |suffix: &str| {
                if suffix.is_empty() {
                    cache_dir.join(&base)
                } else {
                    cache_dir.join(format!("{base}{suffix}"))
                }
            };
            for suffix in ["", ".json", ".part", ".json.part"] {
                match std::fs::remove_file(path(suffix)) {
                    Ok(()) => removed += 1,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        tracing::warn!("Failed to remove invalidated video cache file: {e}");
                    }
                }
            }
        }
        tracing::warn!("Invalidated {removed} video cache files with undecryptable key material");
    }

    /// A video cache. Videos only download on click, but downloading a large
    /// video again on every session is annoying, so videos are persisted in a
    /// cache directory and streamed from there while playing (see
    /// [`crate::state::video_stream`]):
    ///
    /// - encrypted attachments are stored *in their encrypted form* and
    ///   decrypted in-memory while playing, with the key material from the
    ///   event itself;
    /// - plain (unencrypted room) downloads are encrypted with a locally
    ///   generated key on the way into the cache; that key material is itself
    ///   encrypted with a store cipher that only exists in memory and in an
    ///   export encrypted under the OS keyring's store key.
    ///
    /// Either way, no readable video file ever lands on disk, and decrypting
    /// the cached videos requires access to the OS keyring.
    ///
    /// Videos do NOT go via matrix-sdk's media store: that store has no
    /// file size limit by default and round-trips the whole blob through the
    /// encrypted sqlite state db, which measured at over a minute each way
    /// (AEAD of the full blob + BLOB I/O) for a 300MB video.
    #[derive(Clone, Debug)]
    pub struct VideoCache {
        cache: MediaCache<String, LoadedVideo>,
        /// Where the video files are persisted.
        cache_dir: Arc<PathBuf>,
        cipher: SidecarCipher,
        media_processor: crate::state::media_fetcher::MediaProcessor,
    }

    impl VideoCache {
        pub fn new(
            _client: Client,
            media_processor: crate::state::media_fetcher::MediaProcessor,
            store_key: [u8; 32],
            cache_dir: &Path,
        ) -> Self {
            // Stale caches of earlier versions: `videos` once held *decrypted*
            // files (indistinguishable from today's ciphertext when read
            // back), `encrypted_videos` was a short-lived directory name.
            // Remove them so the (large) files can't be misinterpreted and
            // the disk space is reclaimed.
            for obsolete in ["videos", "encrypted_videos"] {
                let obsolete_dir = cache_dir.join(obsolete);
                if obsolete_dir.exists()
                    && let Err(e) = std::fs::remove_dir_all(&obsolete_dir)
                {
                    tracing::warn!("Failed to remove obsolete video cache dir: {e}");
                }
            }

            // The `v2` suffix is the cache format version: bump it whenever
            // the on-disk representation changes in an incompatible way.
            let cache_dir = cache_dir.join("videos-v2");
            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                tracing::warn!("Failed to create video cache dir: {e}");
            }

            let cipher = match SidecarCipher::new(&cache_dir, &store_key) {
                Ok((cipher, restore)) => {
                    if restore == CipherRestore::Rotated {
                        // The store key the sidecar cipher export was
                        // encrypted under is gone (e.g. regenerated on a
                        // fresh login): every entry with locally encrypted
                        // key material is permanently unreadable. Invalidate
                        // those entries in one go instead of letting each of
                        // them fail on first click.
                        invalidate_stale_sidecars(&cache_dir);
                    }
                    cipher
                }
                Err(e) => {
                    // Without a cipher, locally encrypted files can neither be
                    // created nor read; encryption failures must be loud.
                    panic!("Failed to set up the video cache cipher: {e}");
                }
            };

            Self {
                cache: MediaCache::new(_client),
                cache_dir: Arc::new(cache_dir),
                cipher,
                media_processor,
            }
        }

        /// Removes the cached files of a video, e.g. after they failed to
        /// play, so the next load re-downloads instead of being stuck with a
        /// broken cache entry.
        fn evict(&self, key: &str) {
            let paths = [
                self.cached_file_path(key),
                self.cached_info_path(key),
                self.cached_file_path(key).with_extension("part"),
                self.cached_info_path(key).with_extension("json.part"),
            ];

            for path in paths {
                if !path.exists() {
                    continue;
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => tracing::info!(video_key = %key, "Evicted video cache file"),
                    Err(e) => {
                        tracing::warn!("Failed to evict broken video cache file: {e}")
                    }
                }
            }
        }

        pub fn get(&self, key: &String) -> Option<MediaState<LoadedVideo>> {
            self.cache.get(key)
        }

        /// Where the video file for this key is persisted.
        fn cached_file_path(&self, key: &str) -> PathBuf {
            let hex: String = key.bytes().map(|b| format!("{b:02x}")).collect();
            self.cache_dir.join(hex)
        }

        /// Where the key material of a *locally* encrypted (plain source)
        /// video lives. Encrypted attachments don't need this: their key
        /// material is part of the event.
        fn cached_info_path(&self, key: &str) -> PathBuf {
            let hex: String = key.bytes().map(|b| format!("{b:02x}")).collect();
            self.cache_dir.join(format!("{hex}.json"))
        }

        /// Reads and decrypts the locally generated key material of a
        /// plain-source video.
        fn load_cached_info(
            &self,
            key: &str,
            uri: &ruma::OwnedMxcUri,
        ) -> Result<ruma::events::room::EncryptedFile, String> {
            let info_path = self.cached_info_path(key);
            let info = std::fs::read(&info_path)
                .ok()
                .and_then(|bytes| self.cipher.decrypt_info(&bytes).ok())
                .ok_or_else(|| {
                    format!("missing or undecryptable key material for cached video {key}")
                })?;

            Ok(ruma::events::room::EncryptedFile::new(
                uri.clone(),
                info.encryption_info,
                info.hashes,
            ))
        }

        /// Downloads (or reuses) the video's file and plays it from disk,
        /// streaming it through the in-memory decryptor -- for encrypted
        /// attachments with the key material from the event, for plain
        /// downloads with a locally generated key.
        async fn load(
            &self,
            key: &str,
            source: &MediaSource,
        ) -> Result<MediaState<LoadedVideo>, String> {
            // Plain downloads are persisted encrypted with a locally
            // generated key, stored in a sidecar json file.
            let info_path: Option<PathBuf> = match source {
                MediaSource::Encrypted(_) => None,
                MediaSource::Plain(_) => Some(self.cached_info_path(key)),
            };
            let disk_path = self.cached_file_path(key);

            let have_cached =
                disk_path.is_file() && info_path.as_ref().is_none_or(|path| path.is_file());

            if !have_cached {
                let download_started = std::time::Instant::now();
                let part_path = disk_path.with_extension("part");

                if let Some(info_path) = &info_path
                    && disk_path.is_file()
                    && !info_path.is_file()
                {
                    // Stale ciphertext from an interrupted download whose key
                    // material never got written is useless, start fresh.
                    tracing::warn!(video_key = %key, "Removing orphaned video cache file");
                    std::fs::remove_file(&disk_path).map_err(|e| e.to_string())?;
                }

                let downloaded = self
                    .media_processor
                    .download_media_file_to_cache(source, &part_path)
                    .await?;
                tracing::info!(
                    video_key = %key,
                    elapsed_ms = download_started.elapsed().as_millis() as u64,
                    "video downloaded to cache"
                );

                if let Some(info_path) = &info_path
                    && let crate::state::media_fetcher::DownloadedFile::LocallyEncrypted(info) =
                        &downloaded
                {
                    // The key material is stored last: an interrupted download
                    // leaves a ciphertext without usable keys, which is
                    // detected and cleaned up above. It is encrypted with the
                    // cache's sidecar cipher, whose export is only decryptable
                    // with the OS keyring's store key.
                    let encrypted_info = self.cipher.encrypt_info(info)?;
                    let part_info = info_path.with_extension("json.part");
                    std::fs::write(&part_info, encrypted_info).map_err(|e| e.to_string())?;
                    tokio::fs::rename(&part_info, info_path)
                        .await
                        .map_err(|e| e.to_string())?;
                }

                // Rename so concurrent/partial loads never see the file
                // before it is complete.
                if let Err(e) = tokio::fs::rename(&part_path, &disk_path).await {
                    tracing::error!("Failed to move video cache file into place: {e}");
                    if let Err(e) = tokio::fs::remove_file(&part_path).await {
                        tracing::warn!("Failed to remove partial video cache file: {e}");
                    }
                    return Err(e.to_string());
                }
            } else {
                tracing::info!(video_key = %key, "video loaded from disk cache");
            }

            // The decryption info: the attachment info from the event for
            // encrypted sources, the locally generated material for plain ones.
            let encryption = match source {
                MediaSource::Encrypted(file) => Some(file.as_ref().clone()),
                MediaSource::Plain(uri) => Some(self.load_cached_info(key, uri)?),
            };

            let stream_started = std::time::Instant::now();
            let (video, feeder) = crate::state::video_stream::open_stream(
                &disk_path,
                encryption.as_ref(),
                "autoaudiosink sync=true",
            )
            .map_err(|e| e.to_string())?;
            tracing::info!(
                video_key = %key,
                elapsed_ms = stream_started.elapsed().as_millis() as u64,
                "video gstreamer pipeline created"
            );
            video.set_paused(true);
            video.set_looping(true);

            Ok(MediaState::Loaded(Arc::new((
                Arc::new(video),
                Some(feeder),
            ))))
        }
    }

    impl CacheLoadingExt<MediaSource> for VideoCache {
        async fn load_content(&self, source: &MediaSource) -> CacheLoadingResult {
            use matrix_sdk::media::UniqueKey;

            let key = &source.unique_key();
            let media_res = MediaLoaded::Video { key: key.clone() };

            if self.cache.get(key).is_some() {
                return CacheLoadingResult {
                    loaded: media_res,
                    status: CacheResultStatus::CacheHit,
                };
            }

            self.cache.cache.insert(key.clone(), MediaState::Loading);

            let mut status = CacheResultStatus::Success;
            let res = match self.load(key, source).await {
                Ok(loaded) => loaded,
                Err(e) => {
                    tracing::error!("Failed to load video: {e}");
                    // Evict the cache entry: a broken entry must not brick the
                    // video, the next load re-downloads instead.
                    self.evict(key);
                    status = CacheResultStatus::Failure(e);
                    MediaState::Failed
                }
            };

            self.cache.cache.insert(key.clone(), res);

            CacheLoadingResult {
                loaded: media_res,
                status,
            }
        }
    }

    pub type ImageCache = MediaCache<String, (ImageHandle, Vec<u8>, Option<(u32, u32)>)>;

    impl CacheLoadingExt<MediaSource> for ImageCache {
        async fn load_content(&self, source: &MediaSource) -> CacheLoadingResult {
            use matrix_sdk::media::UniqueKey;

            let key = &source.unique_key();
            let media_res = MediaLoaded::Video { key: key.clone() };

            if self.cache.get(key).is_some() {
                return CacheLoadingResult {
                    loaded: media_res,
                    status: CacheResultStatus::CacheHit,
                };
            }

            self.cache.insert(key.clone(), MediaState::Loading);

            let request = &MediaRequestParameters {
                source: source.clone(),
                format: MediaFormat::File,
            };

            let mut status = CacheResultStatus::Success;
            let res = match self.client.media().get_media_content(request, true).await {
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
                    MediaState::Loaded(Arc::new((
                        ImageHandle::from_bytes(bytes.clone()),
                        bytes,
                        dimensions,
                    )))
                }
                Err(e) => {
                    tracing::error!("Failed to fetch media: {e}");
                    status = CacheResultStatus::Failure(e.to_string());
                    MediaState::Failed
                }
            };

            self.cache.insert(key.clone(), res);

            CacheLoadingResult {
                loaded: media_res,
                status,
            }
        }
    }
    #[cfg(test)]
    #[allow(clippy::unwrap_used)]
    mod tests {
        use super::*;

        #[test]
        fn sidecar_cipher_persists_and_wraps() {
            use matrix_sdk_base::crypto::{AttachmentEncryptor, MediaEncryptionInfo};
            use ruma::events::room::EncryptedFileInfo;
            use std::io::Read as _;

            let dir = std::env::temp_dir().join("deplace-cipher-test");
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).unwrap();

            // Key material, as produced when caching a plain video.
            let mut plaintext = b"plaintext".as_slice();
            let mut data = Vec::new();
            let mut encryptor = AttachmentEncryptor::new(&mut plaintext);
            encryptor.read_to_end(&mut data).unwrap();
            let info = encryptor.finish();

            let store_key = [7u8; 32];

            // First creation persists the cipher, a second load restores it
            // from the encrypted export.
            let (cipher, created) = SidecarCipher::new(&dir, &store_key).unwrap();
            assert_eq!(created, CipherRestore::Created);
            let (cipher_again, restored) = SidecarCipher::new(&dir, &store_key).unwrap();
            assert_eq!(restored, CipherRestore::Restored);
            // The export file exists and is not usable as-is.
            let export = std::fs::read(dir.join(".sidecar-cipher")).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&export).unwrap_or_default();

            let encrypted = cipher.encrypt_info(&info).unwrap();
            // The sidecar bytes are opaque ciphertext.
            assert!(!serde_json::from_slice::<MediaEncryptionInfo>(&encrypted).is_ok());
            assert_ne!(json, serde_json::to_value(&encrypted).unwrap());

            let decrypted: MediaEncryptionInfo = cipher_again.decrypt_info(&encrypted).unwrap();
            // The iv of the encryption info makes the two infos comparable.
            let EncryptedFileInfo::V2(decrypted_info) = decrypted.encryption_info else {
                panic!("v2 encryption info expected");
            };
            let EncryptedFileInfo::V2(info) = info.encryption_info else {
                panic!("v2 encryption info expected");
            };
            assert_eq!(decrypted_info.k, info.k);
            assert_eq!(decrypted_info.iv, info.iv);

            // A wrong store key rotates the cipher: a fresh one replaces the
            // existing one and reports the rotation, leaving the old sidecars
            // permanently undecryptable.
            let (wrong, rotated) = SidecarCipher::new(&dir, &[9u8; 32]).unwrap();
            assert_eq!(rotated, CipherRestore::Rotated);
            assert!(wrong.decrypt_info(&encrypted).is_err());
        }

        #[test]
        fn invalidation_removes_only_sidecar_entries() {
            let dir = std::env::temp_dir().join("deplace-sidecar-invalidation-test");
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).unwrap();

            // A plain-source entry: ciphertext plus sidecar key material.
            let plain = dir.join("plain");
            std::fs::write(&plain, b"ciphertext").unwrap();
            let plain_info = dir.join("plain.json");
            std::fs::write(&plain_info, b"sidecar").unwrap();
            // A partially downloaded plain-source entry.
            let part = dir.join("partial.part");
            std::fs::write(&part, b"ciphertext").unwrap();
            let part_info = dir.join("partial.json.part");
            std::fs::write(&part_info, b"sidecar").unwrap();
            // An encrypted-attachment entry: no sidecar (its key material is
            // part of the event), so it stays valid across a rotation.
            let encrypted = dir.join("encrypted");
            std::fs::write(&encrypted, b"ciphertext").unwrap();

            invalidate_stale_sidecars(&dir);

            assert!(!plain.is_file());
            assert!(!plain_info.is_file());
            assert!(!part.is_file());
            assert!(!part_info.is_file());
            assert!(encrypted.is_file());
        }
    }
}
