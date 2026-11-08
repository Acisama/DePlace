use std::{
    borrow::Cow,
    hash::Hash,
    io::{Read, Write as _},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

use dashmap::{DashMap, mapref::entry::Entry};
use futures::{
    FutureExt, StreamExt,
    future::{BoxFuture, Shared},
};
use indexmap::IndexMap;
use matrix_sdk::{
    Client,
    media::{MediaFormat, MediaRequestParameters, UniqueKey},
    reqwest::Client as ReqwestClient,
};
use ruma::{
    api::{
        Metadata, OutgoingRequestExt,
        auth_scheme::SendAccessToken,
        client::{authenticated_media, media},
    },
    events::room::{EncryptedFileHashAlgorithm, MediaSource},
    exports::http,
};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use tokio::sync::watch::{self, Receiver, Sender};

/// A fetch for a given key that is already running. Additional callers for the same
type SharedFetch = Shared<BoxFuture<'static, Result<Arc<Vec<u8>>, Arc<str>>>>;

/// Progress entries in the order they were started. Restarting a key moves it to the end,
/// so iteration order is always start order.
type ProgressMap<V> = Arc<Mutex<IndexMap<String, V>>>;

/// Locks the mutex, recovering the data if a previous holder panicked.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// The event's sha256 hash of a ciphertext source, which the download loop
/// verifies while writing the file. Playback streams the file through a
/// seekable decryptor -- a seek must not read the skipped bytes, so the
/// streaming path cannot verify the hash of the whole ciphertext anymore;
/// the download is the natural place for the check.
fn expected_ciphertext_hash(source: &MediaSource) -> Option<[u8; 32]> {
    let MediaSource::Encrypted(file) = source else {
        return None;
    };

    match file.hashes.get(&EncryptedFileHashAlgorithm::Sha256) {
        Some(ruma::events::room::EncryptedFileHash::Sha256(hash)) => {
            Some(hash.clone().into_inner())
        }
        _ => None,
    }
}

#[derive(Clone)]
pub struct MediaProcessor {
    client: Client,
    /// The client media is requested and streamed over: the SDK's settings
    /// minus its baked-in 30s total-request timeout (see
    /// [`MediaProcessor::new`]).
    streaming_client: ReqwestClient,

    downloads: ProgressMap<DownloadProgress>,
    /// Maps download keys to their corresponding file paths, also ordered by start time.
    download_paths: ProgressMap<PathBuf>,

    compressions: ProgressMap<CompressionProgress>,

    /// Fetches currently in flight, keyed the same way as `progresses`, so a second
    /// request for the same media piggybacks on the first instead of duplicating it.
    in_flight: Arc<DashMap<String, SharedFetch>>,

    /// Bumped whenever any download/compression progress changes, so the UI can wake up
    /// and re-check the (cheap) progress hashes even though nothing else is happening.
    tick: Sender<u64>,
}

impl std::fmt::Debug for MediaProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaProcessor").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub enum DownloadProgress {
    Downloading {
        started_at: SystemTime,
        total_bytes: Arc<AtomicU64>,
        downloaded_bytes: Arc<AtomicU64>,
    },
    Completed {
        started_at: SystemTime,
        completed_at: SystemTime,
    },
    Failed {
        started_at: SystemTime,
        failed_at: SystemTime,
        error: Arc<String>,
    },
}

impl DownloadProgress {
    pub fn new(started_at: SystemTime) -> Self {
        DownloadProgress::Downloading {
            started_at,
            total_bytes: Arc::new(AtomicU64::new(0)),
            downloaded_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn start_time(&self) -> SystemTime {
        match self {
            DownloadProgress::Downloading { started_at, .. } => *started_at,
            DownloadProgress::Completed { started_at, .. } => *started_at,
            DownloadProgress::Failed { started_at, .. } => *started_at,
        }
    }

    pub fn completed(started_at: SystemTime) -> Self {
        DownloadProgress::Completed {
            started_at,
            completed_at: SystemTime::now(),
        }
    }

    pub fn failed(started_at: SystemTime, error: Arc<String>) -> Self {
        DownloadProgress::Failed {
            started_at,
            failed_at: SystemTime::now(),
            error,
        }
    }

    pub fn set_total_bytes(&self, bytes: u64) {
        if let DownloadProgress::Downloading { total_bytes, .. } = self {
            total_bytes.update(Ordering::Relaxed, Ordering::Relaxed, |_| bytes);
        }
    }

    pub fn add_to_downloaded_bytes(&self, bytes: u64) {
        if let DownloadProgress::Downloading {
            downloaded_bytes, ..
        } = self
        {
            downloaded_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    pub fn completeness(&self) -> ProgressCompleteness {
        match self {
            DownloadProgress::Downloading {
                total_bytes,
                downloaded_bytes,
                ..
            } => {
                let downloaded_bytes = downloaded_bytes.load(Ordering::Relaxed);
                if downloaded_bytes == 0 {
                    ProgressCompleteness::NotStarted
                } else {
                    ProgressCompleteness::InProgress(
                        downloaded_bytes as f32 / total_bytes.load(Ordering::Relaxed) as f32,
                    )
                }
            }
            DownloadProgress::Completed { .. } => ProgressCompleteness::Completed,
            DownloadProgress::Failed { .. } => ProgressCompleteness::Failed,
        }
    }
}

impl Hash for DownloadProgress {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            DownloadProgress::Downloading {
                total_bytes,
                downloaded_bytes,
                ..
            } => {
                total_bytes.load(Ordering::Relaxed).hash(state);
                downloaded_bytes.load(Ordering::Relaxed).hash(state);
            }
            DownloadProgress::Completed { .. } => 2.hash(state),
            DownloadProgress::Failed { .. } => 3.hash(state),
        }
    }
}

#[derive(Debug, Clone)]
pub enum CompressionProgress {
    Compressing {
        started_at: SystemTime,
        total_bytes: u64,
        compressed_bytes: Arc<AtomicU64>,
    },
    Completed {
        started_at: SystemTime,
        completed_at: SystemTime,
    },
    Failed {
        started_at: SystemTime,
        failed_at: SystemTime,
        error: Arc<String>,
    },
}

impl CompressionProgress {
    pub fn new(started_at: SystemTime, total_bytes: u64) -> Self {
        CompressionProgress::Compressing {
            started_at,
            total_bytes,
            compressed_bytes: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn start_time(&self) -> SystemTime {
        match self {
            CompressionProgress::Compressing { started_at, .. } => *started_at,
            CompressionProgress::Completed { started_at, .. } => *started_at,
            CompressionProgress::Failed { started_at, .. } => *started_at,
        }
    }

    pub fn add_to_compressed_bytes(&self, bytes: u64) {
        if let CompressionProgress::Compressing {
            compressed_bytes, ..
        } = self
        {
            compressed_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
    }

    pub fn completeness(&self) -> ProgressCompleteness {
        match self {
            CompressionProgress::Compressing {
                total_bytes,
                compressed_bytes,
                ..
            } => {
                let compressed_bytes = compressed_bytes.load(Ordering::Relaxed);
                if compressed_bytes == 0 {
                    ProgressCompleteness::NotStarted
                } else {
                    ProgressCompleteness::InProgress(compressed_bytes as f32 / *total_bytes as f32)
                }
            }
            CompressionProgress::Completed { .. } => ProgressCompleteness::Completed,
            CompressionProgress::Failed { .. } => ProgressCompleteness::Failed,
        }
    }
}

impl Hash for CompressionProgress {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            CompressionProgress::Compressing {
                total_bytes,
                compressed_bytes,
                ..
            } => {
                total_bytes.hash(state);
                compressed_bytes.load(Ordering::Relaxed).hash(state);
            }
            CompressionProgress::Completed { .. } => 2.hash(state),
            CompressionProgress::Failed { .. } => 3.hash(state),
        }
    }
}

#[derive(Debug, Clone, Hash)]
pub enum Progress {
    Download(DownloadProgress),
    Compression(CompressionProgress),
}

#[derive(Debug, Clone)]
pub enum ProgressCompleteness {
    NotStarted,
    InProgress(f32),
    Completed,
    Failed,
}

impl Hash for ProgressCompleteness {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ProgressCompleteness::NotStarted => 0.hash(state),
            ProgressCompleteness::InProgress(float) => float.to_bits().hash(state),
            ProgressCompleteness::Completed => 1.hash(state),
            ProgressCompleteness::Failed => 2.hash(state),
        }
    }
}

/// The result of downloading a file into the video cache.
#[derive(Debug)]
pub enum DownloadedFile {
    /// The source was an encrypted attachment: its raw ciphertext was stored,
    /// the key material lives in the event itself.
    Ciphertext,
    /// The source was a plain download: the file was encrypted with a locally
    /// generated key so nothing readable lands on disk. The info must be
    /// persisted by the caller for later decryption.
    LocallyEncrypted(matrix_sdk_base::crypto::MediaEncryptionInfo),
}

/// A custom media fetch for the matrix client, notifying about download progress
impl MediaProcessor {
    pub fn new(client: &Client) -> Self {
        let (tick, _) = watch::channel(0);

        // The SDK bakes a 30s total-request timeout (its
        // `DEFAULT_REQUEST_TIMEOUT`) into its inner reqwest client, which
        // also aborts any *response body* that streams for longer -- a
        // video on a slow connection can easily exceed that. Media
        // downloads use a client without an overall deadline instead; a
        // read timeout still notices a connection that goes quiet
        // mid-download instead of hanging forever.
        let streaming_client = matrix_sdk::reqwest::Client::builder()
            // Mirror the SDK's client settings (BCP 195, same user agent).
            .min_tls_version(matrix_sdk::reqwest::tls::Version::TLS_1_2)
            .user_agent("matrix-rust-sdk")
            .read_timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("building the media download client must not fail");

        Self {
            client: client.clone(),
            downloads: Arc::new(Mutex::new(IndexMap::new())),
            download_paths: Arc::new(Mutex::new(IndexMap::new())),

            streaming_client,

            compressions: Arc::new(Mutex::new(IndexMap::new())),
            in_flight: Arc::new(DashMap::new()),
            tick,
        }
    }

    /// Subscribes to a tick that fires whenever any download/compression progress
    /// changes, so the UI can wake up and re-render even though nothing else happened.
    pub fn progress_tick(&self) -> Receiver<u64> {
        self.tick.subscribe()
    }

    fn bump_tick(&self) {
        self.tick.send_modify(|v| *v = v.wrapping_add(1));
    }

    pub fn add_relevant_source(&self, source: &MediaRequestParameters, path: PathBuf) {
        lock(&self.download_paths).insert(source.unique_key(), path);
    }

    /// Records a terminal state for a manual download if `fetch_media_content` never
    /// ran for it (which happens when matrix-sdk serves the content straight from its
    /// own media cache instead of using the fetcher)
    pub fn ensure_download_recorded(
        &self,
        request: &MediaRequestParameters,
        started_at: SystemTime,
        result: &std::result::Result<(), String>,
    ) {
        let key = request.unique_key();
        let mut downloads = lock(&self.downloads);

        if downloads.contains_key(&key) {
            return;
        }

        let progress = match result {
            Ok(()) => DownloadProgress::completed(started_at),
            Err(e) => DownloadProgress::failed(started_at, Arc::new(e.clone())),
        };
        downloads.insert(key, progress);
        drop(downloads);
        self.bump_tick();
    }

    // pub fn current_progress(&self) -> Option<Progress> {}

    pub fn start_compression(&self, path: PathBuf, total: u64) -> CompressionProgress {
        let progress = CompressionProgress::new(SystemTime::now(), total);
        let mut compressions = lock(&self.compressions);

        let key = path.to_string_lossy().into_owned();

        // Remove first so a restarted compression moves to the end instead of keeping its old slot
        compressions.shift_remove(&key);
        compressions.insert(key, progress.clone());
        drop(compressions);
        self.bump_tick();

        progress
    }

    pub fn complete_compression(&self, path: PathBuf, started_at: SystemTime) {
        lock(&self.compressions).insert(
            path.to_string_lossy().into_owned(),
            CompressionProgress::Completed {
                started_at,
                completed_at: SystemTime::now(),
            },
        );
        self.bump_tick();
    }

    pub fn fail_compression(&self, path: PathBuf, started_at: SystemTime, error: Arc<String>) {
        lock(&self.compressions).insert(
            path.to_string_lossy().into_owned(),
            CompressionProgress::Failed {
                started_at,
                failed_at: SystemTime::now(),
                error,
            },
        );
        self.bump_tick();
    }

    /// Wakes up the UI to re-check progress hashes. Called by `zip_folder` after each
    /// file is written, since compression progress is mutated directly on the shared
    /// `CompressionProgress` handle rather than through a `MediaProcessor` method.
    pub fn notify_progress(&self) {
        self.bump_tick();
    }

    async fn execute_and_stream(
        &self,
        http_request: http::Request<Vec<u8>>,
        progress: &DownloadProgress,
    ) -> matrix_sdk::Result<Vec<u8>, matrix_sdk::Error> {
        // Deconstruct the HTTP request to build a Reqwest request
        let (parts, _body) = http_request.into_parts();
        let mut reqwest_builder = self
            .streaming_client
            .request(parts.method, parts.uri.to_string());

        for (name, value) in parts.headers {
            if let Some(name) = name {
                reqwest_builder = reqwest_builder.header(name, value);
            }
        }

        // Execute the request
        let response = reqwest_builder
            .send()
            .await
            .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?
            .error_for_status()
            .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?;

        if let Some(total) = response.content_length() {
            progress.set_total_bytes(total);
        }

        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();

        // Stream the body and update progress
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?;
            bytes.extend_from_slice(&chunk);
            progress.add_to_downloaded_bytes(chunk.len() as u64);
            self.bump_tick();
        }

        Ok(bytes)
    }

    /// Does the actual network fetch (and decryption, if needed) for a single media
    /// request. Only ever called once per key at a time - see `fetch_media_content`,
    /// which de-duplicates concurrent requests for the same key before this runs.
    async fn do_fetch(
        &self,
        client: Client,
        request: MediaRequestParameters,
        progress: &DownloadProgress,
    ) -> matrix_sdk::Result<Vec<u8>, matrix_sdk::Error> {
        let supported_versions = client.supported_versions().await?;
        let use_auth = authenticated_media::get_content::v1::Request::PATH_BUILDER
            .is_supported(&supported_versions);

        let homeserver = client.homeserver();
        let token = client.access_token();
        let token_str = token.as_deref().unwrap_or("");
        let auth = SendAccessToken::IfRequired(token_str);

        match &request.source {
            MediaSource::Encrypted(file) => {
                let http_req = if use_auth {
                    authenticated_media::get_content::v1::Request::from_uri(&file.url)?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?
                } else {
                    #[allow(deprecated)]
                    media::get_content::v3::Request::from_url(&file.url)?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?
                };

                let fetch_started = std::time::Instant::now();
                let content = self.execute_and_stream(http_req, progress).await?;
                tracing::info!(
                    media_key = %request.source.unique_key(),
                    compressed_bytes = content.len(),
                    elapsed_ms = fetch_started.elapsed().as_millis() as u64,
                    "media download finished"
                );

                let decrypt_started = std::time::Instant::now();
                let mut cursor = std::io::Cursor::new(content.clone());
                let mut reader = matrix_sdk_base::crypto::AttachmentDecryptor::new(
                    &mut cursor,
                    file.as_ref().clone().into(),
                )?;

                let mut decrypted = Vec::with_capacity(content.len());
                reader.read_to_end(&mut decrypted)?;
                tracing::info!(
                    media_key = %request.source.unique_key(),
                    bytes = decrypted.len(),
                    elapsed_ms = decrypt_started.elapsed().as_millis() as u64,
                    "media decryption finished"
                );

                Ok(decrypted)
            }

            MediaSource::Plain(uri) => {
                let http_req = if let MediaFormat::Thumbnail(settings) = &request.format {
                    if use_auth {
                        let mut req =
                            authenticated_media::get_content_thumbnail::v1::Request::from_uri(
                                uri,
                                settings.width,
                                settings.height,
                            )?;
                        req.method = Some(settings.method.clone());
                        req.animated = Some(settings.animated);
                        req.try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?
                    } else {
                        #[allow(deprecated)]
                        let mut req = media::get_content_thumbnail::v3::Request::from_url(
                            uri,
                            settings.width,
                            settings.height,
                        )?;

                        #[allow(deprecated)]
                        {
                            req.method = Some(settings.method.clone());
                            req.animated = Some(settings.animated);
                        }

                        req.try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?
                    }
                } else if use_auth {
                    authenticated_media::get_content::v1::Request::from_uri(uri)?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?
                } else {
                    #[allow(deprecated)]
                    media::get_content::v3::Request::from_url(uri)?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?
                };

                let fetch_started = std::time::Instant::now();
                let content = self.execute_and_stream(http_req, progress).await?;
                tracing::info!(
                    media_key = %request.source.unique_key(),
                    bytes = content.len(),
                    elapsed_ms = fetch_started.elapsed().as_millis() as u64,
                    "media download finished"
                );
                Ok(content)
            }
        }
    }

    /// Downloads the full file of a media source into `dest` for the video
    /// cache, registering a [`DownloadProgress`] under the same key regular
    /// file fetches use (so the UI can show it). Encrypted attachments are
    /// stored as raw ciphertext, plain downloads are encrypted with a locally
    /// generated key on the fly.
    pub async fn download_media_file_to_cache(
        &self,
        source: &MediaSource,
        dest: &std::path::Path,
    ) -> Result<DownloadedFile, String> {
        // Plain downloads are encrypted locally so no readable file lands in
        // the cache; encrypted attachments are already ciphertext.
        let encrypt = matches!(source, MediaSource::Plain(_));

        let request = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::File,
        };

        let supported_versions = self
            .client
            .supported_versions()
            .await
            .map_err(|e| e.to_string())?;
        let use_auth = authenticated_media::get_content::v1::Request::PATH_BUILDER
            .is_supported(&supported_versions);

        let homeserver = self.client.homeserver();
        let token = self.client.access_token();
        let token_str = token.as_deref().unwrap_or("");
        let auth = SendAccessToken::IfRequired(token_str);

        let http_req = match source {
            MediaSource::Encrypted(file) => {
                if use_auth {
                    authenticated_media::get_content::v1::Request::from_uri(&file.url)
                        .map_err(|e| e.to_string())?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| e.to_string())?
                } else {
                    #[allow(deprecated)]
                    media::get_content::v3::Request::from_url(&file.url)
                        .map_err(|e| e.to_string())?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| e.to_string())?
                }
            }
            MediaSource::Plain(uri) => {
                if use_auth {
                    authenticated_media::get_content::v1::Request::from_uri(uri)
                        .map_err(|e| e.to_string())?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| e.to_string())?
                } else {
                    #[allow(deprecated)]
                    media::get_content::v3::Request::from_url(uri)
                        .map_err(|e| e.to_string())?
                        .try_into_http_request::<Vec<u8>>(
                            homeserver.as_str(),
                            auth,
                            Cow::Borrowed(&supported_versions),
                        )
                        .map_err(|e| e.to_string())?
                }
            }
        };

        let started_at = SystemTime::now();
        let progress = DownloadProgress::new(started_at);
        {
            let mut downloads = lock(&self.downloads);
            downloads.shift_remove(&request.unique_key());
            downloads.insert(request.unique_key(), progress.clone());
        }
        self.bump_tick();

        tracing::info!(
            media_key = %source.unique_key(),
            encrypted_source = !encrypt,
            "video cache file download started"
        );

        let result = self
            .stream_to_file(
                http_req,
                &progress,
                dest,
                encrypt,
                expected_ciphertext_hash(source),
            )
            .await;

        tracing::info!(
            media_key = %source.unique_key(),
            elapsed_ms = started_at.elapsed().unwrap_or_default().as_millis() as u64,
            successful = result.is_ok(),
            "video cache file download finished"
        );

        {
            let mut downloads = lock(&self.downloads);
            downloads.insert(
                request.unique_key(),
                match &result {
                    Ok(_) => DownloadProgress::completed(started_at),
                    Err(e) => DownloadProgress::failed(started_at, Arc::new(e.clone())),
                },
            );
        }
        self.bump_tick();

        result.map(|info| match info {
            Some(info) => DownloadedFile::LocallyEncrypted(info),
            None => DownloadedFile::Ciphertext,
        })
    }

    /// Streams the body of the given HTTP request into `dest`, updating the
    /// progress while doing so; with `encrypt`, wraps the writer so a plain
    /// download is encrypted (matrix-attachment style) on the way in,
    /// returning its decryption info. Ciphertext downloads are verified
    /// against `expected_ciphertext_hash` (the event's sha256 hash of the
    /// ciphertext) so corrupted or mismatching downloads never reach the
    /// cache: playback streams decrypted data seekably and no longer passes
    /// every byte through a sequential hash.
    async fn stream_to_file(
        &self,
        http_request: http::Request<Vec<u8>>,
        progress: &DownloadProgress,
        dest: &std::path::Path,
        encrypt: bool,
        expected_ciphertext_hash: Option<[u8; 32]>,
    ) -> Result<Option<matrix_sdk_base::crypto::MediaEncryptionInfo>, String> {
        // Deconstruct the HTTP request to build a Reqwest request
        let (parts, _body) = http_request.into_parts();
        let mut reqwest_builder = self
            .streaming_client
            .request(parts.method, parts.uri.to_string());

        for (name, value) in parts.headers {
            if let Some(name) = name {
                reqwest_builder = reqwest_builder.header(name, value);
            }
        }

        let response = reqwest_builder
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;

        if let Some(total) = response.content_length() {
            progress.set_total_bytes(total);
        }

        let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        let mut stream = response.bytes_stream();

        if encrypt {
            let mut writer = super::local_file_encryption::AttachmentWriter::new(file)
                .map_err(|e| e.to_string())?;

            while let Some(chunk_res) = stream.next().await {
                let chunk = chunk_res.map_err(|e| e.to_string())?;
                writer.write_all(&chunk).map_err(|e| e.to_string())?;
                progress.add_to_downloaded_bytes(chunk.len() as u64);
                self.bump_tick();
            }

            let info = writer.finish().map_err(|e| e.to_string())?;
            Ok(Some(info))
        } else {
            let mut writer = std::io::BufWriter::new(file);
            let mut sha = Sha256::default();

            while let Some(chunk_res) = stream.next().await {
                let chunk = chunk_res.map_err(|e| e.to_string())?;
                writer.write_all(&chunk).map_err(|e| e.to_string())?;
                if expected_ciphertext_hash.is_some() {
                    sha.update(&chunk);
                }
                progress.add_to_downloaded_bytes(chunk.len() as u64);
                self.bump_tick();
            }

            writer.flush().map_err(|e| e.to_string())?;

            if let Some(expected) = expected_ciphertext_hash {
                let digest: [u8; 32] = sha.finalize().into();
                if digest != expected {
                    return Err("downloaded file does not match the announced sha256 hash".into());
                }
            }

            Ok(None)
        }
    }

    /// Progress of the full-file download for the given media source, if one is
    /// running or has finished.
    pub fn file_download_progress(&self, source: &MediaSource) -> Option<DownloadProgress> {
        let request = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::File,
        };
        lock(&self.downloads).get(&request.unique_key()).cloned()
    }

    pub fn current_download(&self) -> Option<DownloadProgress> {
        let all_downloads = lock(&self.downloads);
        let download_keys = lock(&self.download_paths);

        download_keys
            .keys()
            .rev()
            .filter_map(|key| all_downloads.get(key))
            .next()
            .cloned()
    }

    pub fn current_compression(&self) -> Option<CompressionProgress> {
        let compressions = lock(&self.compressions);

        compressions.values().rev().next_back().cloned()
    }
}

impl matrix_sdk::media::MediaFetcher for MediaProcessor {
    fn fetch_media_content<'a>(
        &'a self,
        client: &'a Client,
        request: &'a matrix_sdk::media::MediaRequestParameters,
    ) -> matrix_sdk::BoxFuture<'a, matrix_sdk::Result<Vec<u8>, matrix_sdk::Error>> {
        let key = request.unique_key();

        // Atomically check for (and if absent, register) an in-flight fetch for this
        // key, so two concurrent requests for the same media share one network fetch
        // instead of racing to write into the same `progresses` entry.
        let shared = match self.in_flight.entry(key.clone()) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => {
                let started_at = SystemTime::now();
                let progress = DownloadProgress::new(started_at);
                {
                    let mut progresses = lock(&self.downloads);
                    // Remove first so a refetched key moves to the end instead of keeping its old slot
                    progresses.shift_remove(&key);
                    progresses.insert(key.clone(), progress.clone());
                }
                self.bump_tick();

                let this = self.clone();
                let client = client.clone();
                let request = request.clone();
                let key_for_fut = key.clone();

                let fut: BoxFuture<'static, Result<Arc<Vec<u8>>, Arc<str>>> =
                    Box::pin(async move {
                        let result = this.do_fetch(client, request, &progress).await;
                        this.in_flight.remove(&key_for_fut);

                        lock(&this.downloads).insert(
                            key_for_fut,
                            match &result {
                                Ok(_) => DownloadProgress::completed(started_at),
                                Err(e) => {
                                    DownloadProgress::failed(started_at, Arc::new(e.to_string()))
                                }
                            },
                        );
                        this.bump_tick();

                        result.map(Arc::new).map_err(|e| Arc::from(e.to_string()))
                    });

                let shared = fut.shared();
                entry.insert(shared.clone());
                shared
            }
        };

        Box::pin(async move {
            shared
                .await
                .map(|bytes| (*bytes).clone())
                .map_err(|e| matrix_sdk::Error::UnknownError(anyhow::anyhow!("{e}").into()))
        })
    }
}
