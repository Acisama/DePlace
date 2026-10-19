use std::{
    borrow::Cow,
    collections::BTreeSet,
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
use indexmap::IndexMap;
use futures::{
    FutureExt, StreamExt,
    future::{BoxFuture, Shared},
};
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

/// A fetch for a given key that is already running. Additional callers for the same
type SharedFetch = Shared<BoxFuture<'static, Result<Arc<Vec<u8>>, Arc<str>>>>;

/// Progress entries in the order they were started. Restarting a key moves it to the end,
/// so iteration order is always start order.
type ProgressMap<K> = Arc<Mutex<IndexMap<K, ProgressEntry>>>;

/// Locks the mutex, recovering the data if a previous holder panicked.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[derive(Default, Clone)]
pub struct MediaProcessor {
    progresses: ProgressMap<String>,
    reqwest_client: ReqwestClient,

    /// Keys that are relevant for the ui, so stuff like avatar fetches can be ignored
    relevant_keys: Arc<Mutex<BTreeSet<String>>>,

    compressions: ProgressMap<PathBuf>,

    /// Fetches currently in flight, keyed the same way as `progresses`, so a second
    /// request for the same media piggybacks on the first instead of duplicating it.
    in_flight: Arc<DashMap<String, SharedFetch>>,
}

impl std::fmt::Debug for MediaProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaProcessor").finish_non_exhaustive()
    }
}

#[derive(Default, Debug, Clone)]
pub enum Progress {
    #[default]
    NotStarted,
    Downloading {
        total_bytes: Arc<AtomicU64>,
        downloaded_bytes: Arc<AtomicU64>,
    },
    Completed(SystemTime),
    Failed(SystemTime),
}

impl Hash for Progress {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        match self {
            Progress::NotStarted => 0.hash(hasher),
            Progress::Downloading {
                total_bytes,
                downloaded_bytes,
            } => {
                total_bytes.as_ptr().hash(hasher);
                downloaded_bytes.as_ptr().hash(hasher);
            }
            Progress::Completed(_) => 1.hash(hasher),
            Progress::Failed(_) => 2.hash(hasher),
        }
    }
}

impl ProgressEntry {
    pub fn new(state: Progress, started_at: SystemTime) -> Self {
        Self { state, started_at }
    }

    /// Updates the total bytes of a downloading progress if it is currently downloading, using the given update function.
    pub fn update_total_bytes(&self, update_fn: impl FnMut(u64) -> u64) {
        if let Progress::Downloading { total_bytes, .. } = &self.state {
            total_bytes.update(Ordering::Relaxed, Ordering::Relaxed, update_fn);
        }
    }

    /// Sets the total bytes of a downloading progress if it is currently downloading.
    pub fn set_total_bytes(&self, total_bytes: u64) {
        if let Progress::Downloading {
            total_bytes: total_bytes_ref,
            ..
        } = &self.state
        {
            total_bytes_ref.update(Ordering::Relaxed, Ordering::Relaxed, |_| total_bytes);
        }
    }

    pub fn set_downloaded_bytes(&self, downloaded_bytes: u64) {
        if let Progress::Downloading {
            downloaded_bytes: downloaded_bytes_ref,
            ..
        } = &self.state
        {
            downloaded_bytes_ref.update(Ordering::Relaxed, Ordering::Relaxed, |_| downloaded_bytes);
        }
    }

    /// Updates the downloaded bytes of a downloading progress if it is currently downloading, using the given update function.
    pub fn update_downloaded_bytes(&self, update_fn: impl FnMut(u64) -> u64) {
        if let Progress::Downloading {
            downloaded_bytes, ..
        } = &self.state
        {
            downloaded_bytes.update(Ordering::Relaxed, Ordering::Relaxed, update_fn);
        }
    }

    pub fn downloading_new(started_at: SystemTime) -> Self {
        Self {
            state: Progress::Downloading {
                total_bytes: Arc::new(AtomicU64::new(0)),
                downloaded_bytes: Arc::new(AtomicU64::new(0)),
            },
            started_at,
        }
    }

    pub fn completeness(&self) -> f64 {
        if let Progress::Downloading {
            total_bytes,
            downloaded_bytes,
        } = &self.state
        {
            let total = total_bytes.load(Ordering::Relaxed) as f64;
            let downloaded = downloaded_bytes.load(Ordering::Relaxed) as f64;
            downloaded / total
        } else {
            0.0
        }
    }

    pub fn completed(started_at: SystemTime, success: bool) -> Self {
        Self {
            state: if success {
                Progress::Completed(SystemTime::now())
            } else {
                Progress::Failed(SystemTime::now())
            },
            started_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProgressEntry {
    pub state: Progress,
    pub started_at: SystemTime,
}

impl Hash for ProgressEntry {
    fn hash<H: std::hash::Hasher>(&self, hasher: &mut H) {
        self.state.hash(hasher);
    }
}

/// A custom media fetch for the matrix client, notifying about download progress
impl MediaProcessor {
    pub fn new(client: &Client) -> Self {
        Self {
            progresses: Arc::new(Mutex::new(IndexMap::new())),
            reqwest_client: client.http_client().clone(),
            relevant_keys: Arc::new(Mutex::new(BTreeSet::new())),

            compressions: Arc::new(Mutex::new(IndexMap::new())),
            in_flight: Arc::new(DashMap::new()),
        }
    }

    pub fn add_relevant_source(&self, source: &MediaSource) {
        lock(&self.relevant_keys).insert(source.unique_key());
    }

    // pub fn current_progress(&self) -> Option<Progress> {}

    pub fn start_compression(&self, path: PathBuf) -> ProgressEntry {
        let progress = ProgressEntry::downloading_new(SystemTime::now());
        let mut compressions = lock(&self.compressions);
        // Remove first so a restarted compression moves to the end instead of keeping its old slot
        compressions.shift_remove(&path);
        compressions.insert(path, progress.clone());

        progress
    }

    pub fn finish_compression(&self, path: PathBuf, success: bool, started_at: SystemTime) {
        // Replacing an existing key keeps its position, so the start order is preserved
        lock(&self.compressions).insert(path, ProgressEntry::completed(started_at, success));
    }

    async fn execute_and_stream(
        &self,
        http_request: http::Request<Vec<u8>>,
        progress: &ProgressEntry,
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
            progress.update_total_bytes(|_| total);
        }

        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();

        // Stream the body and update progress
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| matrix_sdk::Error::UnknownError(e.into()))?;
            bytes.extend_from_slice(&chunk);
            progress.update_downloaded_bytes(|cur| cur + chunk.len() as u64);
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
        progress: &ProgressEntry,
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
                let progress = ProgressEntry::downloading_new(started_at);
                {
                    let mut progresses = lock(&self.progresses);
                    // Remove first so a refetched key moves to the end instead of keeping its old slot
                    progresses.shift_remove(&key);
                    progresses.insert(key.clone(), progress.clone());
                }

                let this = self.clone();
                let client = client.clone();
                let request = request.clone();
                let key_for_fut = key.clone();

                let fut: BoxFuture<'static, Result<Arc<Vec<u8>>, Arc<str>>> =
                    Box::pin(async move {
                        let result = this.do_fetch(client, request, &progress).await;
                        this.in_flight.remove(&key_for_fut);

                        lock(&this.progresses).insert(
                            key_for_fut,
                            ProgressEntry::completed(started_at, result.is_ok()),
                        );

                        result
                            .map(Arc::new)
                            .map_err(|e| Arc::from(e.to_string()))
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
