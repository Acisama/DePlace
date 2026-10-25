use std::{
    borrow::Cow,
    hash::Hash,
    io::Read,
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
    events::room::MediaSource,
    exports::http,
};
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

#[derive(Clone)]
pub struct MediaProcessor {
    reqwest_client: ReqwestClient,

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

/// A custom media fetch for the matrix client, notifying about download progress
impl MediaProcessor {
    pub fn new(client: &Client) -> Self {
        let (tick, _) = watch::channel(0);

        Self {
            downloads: Arc::new(Mutex::new(IndexMap::new())),
            download_paths: Arc::new(Mutex::new(IndexMap::new())),
            reqwest_client: client.http_client().clone(),

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
            .reqwest_client
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

                let content = self.execute_and_stream(http_req, progress).await?;

                let mut cursor = std::io::Cursor::new(content.clone());
                let mut reader = matrix_sdk_base::crypto::AttachmentDecryptor::new(
                    &mut cursor,
                    file.as_ref().clone().into(),
                )?;

                let mut decrypted = Vec::with_capacity(content.len());
                reader.read_to_end(&mut decrypted)?;

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

                let content = self.execute_and_stream(http_req, progress).await?;
                Ok(content)
            }
        }
    }

    pub fn current_download(&self) -> Option<DownloadProgress> {
        let all_downloads = lock(&self.downloads);
        let download_keys = lock(&self.download_paths);

        let downloads = download_keys
            .keys()
            .filter_map(|key| all_downloads.get(key));

        downloads
            .rev()
            .find(|p| matches!(p, DownloadProgress::Downloading { .. }))
            .or_else(|| all_downloads.values().next_back())
            .cloned()
    }

    pub fn current_compression(&self) -> Option<CompressionProgress> {
        let compressions = lock(&self.compressions);

        compressions
            .values()
            .rev()
            .find(|p| matches!(p, CompressionProgress::Compressing { .. }))
            .or_else(|| compressions.values().next_back())
            .cloned()
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
            Entry::Occupied(entry) => {
                tracing::debug!("Piggybacking on in-flight media fetch: {}", key);
                entry.get().clone()
            }
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
