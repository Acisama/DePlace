use std::{
    collections::HashMap,
    env::temp_dir,
    fmt::Debug,
    hash::Hash,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use futures::{Stream, stream};
use matrix_sdk::{
    Client,
    ruma::{
        OwnedDeviceId, OwnedRoomId, OwnedUserId,
        api::client::receipt::create_receipt::v3::ReceiptType, events::receipt::ReceiptThread,
    },
};
use media_fetcher::MediaProcessor;
use ruma::{OwnedMxcUri, RoomId};
use tokio::sync::watch::{self, Receiver, Sender};

use crate::{
    APP_NAME,
    audio::AudioThread,
    call::CallRegistry,
    keybinds::Keybinds,
    matrix_api::{
        account_data::{BreadcrumbsContent, DePlaceAccountData, ServerOrderContent},
        matrixrtc::LiveKitRoomManager,
        sync::spawn_initial_load,
    },
    member::ProfileStore,
    notifications::NotificationManager,
    room::{DePlaceRoom, notifications::NotificationCounts, store::RoomStore},
    settings::Settings,
    window_title,
};

#[cfg(feature = "iced")]
use crate::Avatar;

#[derive(Debug, Clone)]
pub struct ImportantPaths {
    pub config_dir: Arc<PathBuf>,
    pub settings_file: Arc<PathBuf>,
    pub keybinds_file: Arc<PathBuf>,
    pub theme_file: Arc<PathBuf>,
    pub structure_file: Arc<PathBuf>,

    pub download_dir: Arc<PathBuf>,
    pub cache_dir: Arc<PathBuf>,
    pub temp_dir: Arc<PathBuf>,
    pub data_dir: Arc<PathBuf>,
    pub log_dir: Arc<PathBuf>,
}

impl ImportantPaths {
    pub fn new() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .ok_or(anyhow::anyhow!("Failed to get config dir"))?
            .join(APP_NAME);
        let download_dir =
            dirs::download_dir().ok_or(anyhow::anyhow!("Failed to get download dir"))?;
        let cache_dir = dirs::cache_dir()
            .ok_or(anyhow::anyhow!("Failed to get cache dir"))?
            .join(APP_NAME);
        let temp_dir = temp_dir();
        let data_dir = dirs::data_dir()
            .ok_or(anyhow::anyhow!("Failed to get data dir"))?
            .join(APP_NAME);
        let log_dir = data_dir.join("logs");

        for path in [&config_dir, &download_dir, &cache_dir, &data_dir, &log_dir] {
            if !path.exists() {
                std::fs::create_dir_all(path)?;
            }
        }

        let settings_file = config_dir.join("config.toml");
        let keybinds_file = config_dir.join("keybinds.toml");
        let theme_file = config_dir.join("theme.toml");
        let structure_file = config_dir.join("structure.toml");

        Ok(Self {
            config_dir: Arc::new(config_dir),
            settings_file: Arc::new(settings_file),
            keybinds_file: Arc::new(keybinds_file),
            theme_file: Arc::new(theme_file),
            structure_file: Arc::new(structure_file),

            download_dir: Arc::new(download_dir),
            cache_dir: Arc::new(cache_dir),
            temp_dir: Arc::new(temp_dir),
            data_dir: Arc::new(data_dir),
            log_dir: Arc::new(log_dir),
        })
    }
}

#[derive(Clone, Debug)]
pub struct UserDevice {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
}

pub mod cache;
pub mod media_fetcher;
pub mod roles;

#[derive(Clone, Debug)]
pub enum ActiveServer {
    Dms,
    Server(DePlaceRoom),
}

impl From<Option<DePlaceRoom>> for ActiveServer {
    fn from(room: Option<DePlaceRoom>) -> Self {
        match room {
            Some(room) => ActiveServer::Server(room),
            None => ActiveServer::Dms,
        }
    }
}

impl ActiveServer {
    pub fn id(&self) -> ActiveServerId {
        match self {
            ActiveServer::Dms => ActiveServerId::Dms,
            ActiveServer::Server(room) => ActiveServerId::Server(room.room_id().to_owned()),
        }
    }

    pub fn is_dms(&self) -> bool {
        matches!(self, ActiveServer::Dms)
    }

    pub fn is_server(&self, server_id: &RoomId) -> bool {
        matches!(self, ActiveServer::Server(room) if room.room_id() == server_id)
    }

    pub fn as_server(&self) -> Option<&DePlaceRoom> {
        match self {
            ActiveServer::Server(room) => Some(room),
            ActiveServer::Dms => None,
        }
    }
}

impl std::hash::Hash for ActiveServer {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ActiveServer::Dms => 0.hash(state),
            ActiveServer::Server(server) => server.room_id().hash(state),
        }
    }
}

impl PartialEq for ActiveServer {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ActiveServer::Dms, ActiveServer::Dms) => true,
            (ActiveServer::Server(room1), ActiveServer::Server(room2)) => {
                room1.room_id() == room2.room_id()
            }
            _ => false,
        }
    }
}

impl ActiveServerId {
    pub fn is_dms(&self) -> bool {
        matches!(self, ActiveServerId::Dms)
    }

    pub fn is_server(&self, server_id: &RoomId) -> bool {
        matches!(self, ActiveServerId::Server(id) if id == server_id)
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
pub enum ActiveServerId {
    Dms,
    Server(OwnedRoomId),
}

impl From<Option<OwnedRoomId>> for ActiveServerId {
    fn from(room_id: Option<OwnedRoomId>) -> Self {
        match room_id {
            Some(id) => ActiveServerId::Server(id),
            None => ActiveServerId::Dms,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) inner: Arc<AppStateInner>,
}

impl Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppState")
    }
}

pub(crate) struct AppStateInner {
    pub client: Client,
    pub own_device: Arc<UserDevice>,
    pub settings: Settings,

    pub(crate) media_processor: MediaProcessor,

    pub(crate) profile_store: Arc<ProfileStore>,
    pub(crate) room_store: Arc<RoomStore>,

    pub(crate) own_display_name: Sender<Option<String>>,
    pub(crate) own_avatar_url: Sender<Option<Arc<OwnedMxcUri>>>,
    #[cfg(feature = "iced")]
    pub(crate) own_avatar_handle: Sender<Avatar>,

    keybinds: Sender<Keybinds>,

    window_title: Sender<String>,
    window_focused: Mutex<bool>,
    important_paths: ImportantPaths,

    notification_manager: NotificationManager,

    /// The app's one audio connection, started when the app starts.
    ///
    /// Everything that wants to make a sound (calls, notification sounds,
    /// whatever else) goes through this. `None` means the sound device would
    /// not open
    audio: Option<AudioThread>,

    /// All MatrixRTC calls the app is currently in, keyed by room id.
    livekit_room_manager: LiveKitRoomManager,
    /// The frontend-facing handles of those calls: state to read,
    /// commands to send.
    call_registry: CallRegistry,

    sync_tick: Sender<u64>,

    pub active_room: Sender<Option<DePlaceRoom>>,
    pub active_server: Sender<ActiveServer>,

    server_order: Sender<Vec<OwnedRoomId>>,

    /// Last accessed servers and rooms, since the data is only
    /// needed as snapshots, it is held in a mutex
    breadcrumbs: Mutex<BreadcrumbsContent>,

    #[cfg(feature = "iced_desktop")]
    thumbnail_cache: cache::ThumbnailCache,

    #[cfg(feature = "iced_desktop")]
    video_cache: cache::VideoCache,

    #[cfg(feature = "iced_desktop")]
    image_cache: cache::ImageCache,
}

impl AppState {
    pub async fn new(
        client: Client,
        own_device: Arc<UserDevice>,
        settings: Settings,
        keybinds: Keybinds,
        important_paths: ImportantPaths,
    ) -> Result<Self> {
        let media_processor = MediaProcessor::new(&client);
        client
            .set_media_fetcher(Arc::new(media_processor.clone()))
            .await;

        let breadcrumbs_content = BreadcrumbsContent::fetch(&client).await;
        let last_server_order = ServerOrderContent::fetch(&client).await;

        let profile_store = Arc::new(ProfileStore::new(&client));
        let room_store =
            Arc::new(RoomStore::new(&client, profile_store.clone(), last_server_order).await);

        let breadcrumbs = Mutex::new(breadcrumbs_content);

        let (last_room_id, dms_last) = {
            let breadcrumbs = breadcrumbs
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());

            let dms_last = breadcrumbs.dms_last;
            let last_room_id = if dms_last {
                breadcrumbs.last_dm_id.clone()
            } else {
                breadcrumbs.recent_rooms.first().cloned()
            };

            (last_room_id, dms_last)
        };

        let active_server = if !dms_last
            && let Some(room_id) = &last_room_id
            && let Some(server) = room_store.get_server_of(room_id)
        {
            ActiveServer::Server(server)
        } else {
            ActiveServer::Dms
        };

        let active_room = last_room_id.and_then(|id| room_store.get_room(&id));

        let (active_room, _) = watch::channel(active_room);
        let (active_server, _) = watch::channel(active_server);

        let server_order_data = ServerOrderContent::fetch(&client).await;
        let (server_order, _) = watch::channel(server_order_data.servers);
        let (sync_tick, _) = watch::channel(0);

        let (window_title, _) = watch::channel(window_title(
            active_room.borrow().clone(),
            active_server.borrow().clone(),
        ));

        let (keybinds, _) = watch::channel(keybinds);

        let audio = AudioThread::start()
            .map_err(|e| {
                tracing::warn!("Could not start audio, the app will be silent: {e:#}");
                e
            })
            .ok();

        let livekit_room_manager: LiveKitRoomManager =
            Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        let (own_display_name, _) = watch::channel(None);
        let (own_avatar_url, _) = watch::channel(None);

        spawn_initial_load(
            profile_store.clone(),
            client.clone(),
            room_store.clone(),
            own_device.clone(),
        );

        Ok(Self {
            inner: Arc::new(AppStateInner {
                #[cfg(feature = "iced_desktop")]
                thumbnail_cache: cache::ThumbnailCache::new(client.clone()),

                #[cfg(feature = "iced_desktop")]
                video_cache: cache::VideoCache::new(client.clone()),

                #[cfg(feature = "iced_desktop")]
                image_cache: cache::ImageCache::new(client.clone()),

                media_processor,
                profile_store,
                room_store,

                window_title,
                window_focused: Mutex::new(false),
                important_paths,

                own_display_name,
                own_avatar_url,
                #[cfg(feature = "iced")]
                own_avatar_handle: Sender::default(),

                notification_manager: NotificationManager::default(),

                audio,

                livekit_room_manager,
                call_registry: CallRegistry::new(),

                client,
                own_device,
                settings,
                keybinds,

                sync_tick,

                active_room,
                active_server,

                server_order,
                breadcrumbs,
            }),
        })
    }

    #[cfg(feature = "iced_desktop")]
    pub fn thumbnail_cache(&self) -> &cache::ThumbnailCache {
        &self.inner.thumbnail_cache
    }

    #[cfg(feature = "iced_desktop")]
    pub fn video_cache(&self) -> &cache::VideoCache {
        &self.inner.video_cache
    }

    #[cfg(feature = "iced_desktop")]
    pub fn image_cache(&self) -> &cache::ImageCache {
        &self.inner.image_cache
    }

    pub fn profile_store_ref(&self) -> &ProfileStore {
        &self.inner.profile_store
    }

    pub fn room_store_ref(&self) -> &RoomStore {
        &self.inner.room_store
    }

    pub fn profile_store(&self) -> Arc<ProfileStore> {
        self.inner.profile_store.clone()
    }

    pub fn room_store(&self) -> Arc<RoomStore> {
        self.inner.room_store.clone()
    }

    pub fn member_update_receiver(&self) -> Receiver<()> {
        self.inner.profile_store.changed.subscribe()
    }

    pub fn room_update_receiver(&self) -> Receiver<()> {
        self.inner.room_store.changed.subscribe()
    }

    pub fn own_display_name(&self) -> Receiver<Option<String>> {
        self.inner.own_display_name.subscribe()
    }

    pub fn set_own_display_name(&self, name: Option<String>) {
        self.inner.own_display_name.send_replace(name);
    }

    pub fn own_avatar_url(&self) -> Receiver<Option<Arc<OwnedMxcUri>>> {
        self.inner.own_avatar_url.subscribe()
    }

    pub fn own_avatar(&self) -> Receiver<Avatar> {
        self.inner.own_avatar_handle.subscribe()
    }

    pub fn keybinds(&self) -> Receiver<Keybinds> {
        self.inner.keybinds.subscribe()
    }

    pub fn notification_manager(&self) -> &NotificationManager {
        &self.inner.notification_manager
    }

    /// The app's audio connection, shared by everything that makes a sound.
    /// `None` if the sound device would not open, sounds are simply skipped
    /// then.
    pub fn audio(&self) -> Option<AudioThread> {
        self.inner.audio.clone()
    }

    /// All MatrixRTC calls the app is currently in, keyed by room id
    pub fn livekit_room_manager(&self) -> LiveKitRoomManager {
        self.inner.livekit_room_manager.clone()
    }

    /// All ongoing calls: state to render, commands to send, plus the
    /// wake channel that fires when a call joins or leaves.
    pub fn call_registry(&self) -> CallRegistry {
        self.inner.call_registry.clone()
    }

    pub fn important_paths(&self) -> &ImportantPaths {
        &self.inner.important_paths
    }

    pub fn window_focused(&self) -> bool {
        *self
            .inner
            .window_focused
            .lock()
            .unwrap_or_else(|posion| posion.into_inner())
    }

    pub fn set_window_focused(&self, focused: bool) {
        if focused {
            tracing::trace!("Window focused");
        } else {
            tracing::trace!("Window unfocused");
        }

        *self
            .inner
            .window_focused
            .lock()
            .unwrap_or_else(|posion| posion.into_inner()) = focused;
    }

    pub fn window_title(&self) -> Receiver<String> {
        self.inner.window_title.subscribe()
    }

    /// Subscribes to a tick that fires on every processed sync response,
    /// regardless of whether it contained anything relevant to the
    /// subscriber. Intended to wake up UI caches so they re-check their
    /// (cheap) hashes against the latest state.
    pub fn sync_tick(&self) -> Receiver<u64> {
        self.inner.sync_tick.subscribe()
    }

    /// Retrieves the `matrix-sdk::Client` of the app
    pub fn client(&self) -> Client {
        self.inner.client.clone()
    }

    pub fn media_processor(&self) -> MediaProcessor {
        self.inner.media_processor.clone()
    }

    /// Retrieves the `Settings` of the app
    pub fn settings(&self) -> Settings {
        self.inner.settings.clone()
    }

    pub fn own_device(&self) -> Arc<UserDevice> {
        self.inner.own_device.clone()
    }

    pub fn own_device_ref(&self) -> &UserDevice {
        &self.inner.own_device
    }

    pub fn own_id(&self) -> OwnedUserId {
        self.inner.own_device.user_id.clone()
    }

    // Getters

    pub fn active_room(&self) -> watch::Receiver<Option<DePlaceRoom>> {
        self.inner.active_room.subscribe()
    }

    pub fn active_server(&self) -> watch::Receiver<ActiveServer> {
        self.inner.active_server.subscribe()
    }

    pub fn server_order(&self) -> watch::Receiver<Vec<OwnedRoomId>> {
        self.inner.server_order.subscribe()
    }

    /// Returns a snapshot copy of the breadcrumbs
    pub fn breadcrumbs(&self) -> BreadcrumbsContent {
        self.inner
            .breadcrumbs
            .lock()
            .expect("breadcrumbs mutex poisoned")
            .clone()
    }

    // Mutators
    pub(crate) fn bump_sync_tick(&self) {
        self.inner.sync_tick.send_modify(|v| *v = v.wrapping_add(1));
    }

    /// Mutates the breadcrumbs in-place using a closure. This function is private since
    /// breadcrumbs should not be updated manually.
    fn update_breadcrumbs<F>(&self, f: F)
    where
        F: FnOnce(&mut BreadcrumbsContent),
    {
        let mut guard = self
            .inner
            .breadcrumbs
            .lock()
            .expect("breadcrumbs mutex poisoned");
        f(&mut guard);
    }

    /// Sets the active server. Also sets the active room to the server's room if `change_room` is true.
    pub async fn set_active_server(&self, server: ActiveServer, change_room: bool) {
        tracing::trace!(
            "Setting active server: {:?}",
            server
                .as_server()
                .map(|r| r.room_id().as_str())
                .unwrap_or("Dms")
        );
        let mut server_changed = false;

        // change the server
        self.inner.active_server.send_if_modified(|cur| {
            let changed = cur != &server;
            if changed {
                *cur = server.clone();
                server_changed = true;
            }
            changed
        });

        // if the server was changed, also change the room
        if server_changed {
            self.recalculate_title();

            let server_id = server.id();
            let breadcrumbs = self.breadcrumbs();

            let new_room = match server_id {
                ActiveServerId::Server(id) => breadcrumbs
                    .last_space_ids
                    .get(&id)
                    .cloned()
                    .and_then(|id| self.inner.room_store.get_room(&id))
                    .or_else(|| self.inner.room_store.get_first_child_of(&id)),
                ActiveServerId::Dms => breadcrumbs
                    .last_dm_id
                    .clone()
                    .and_then(|id| self.inner.room_store.get_room(&id))
                    .or_else(|| self.inner.room_store.dm_rooms().iter().next().cloned()),
            };

            if change_room {
                self.set_active_room(new_room).await;
            }
        }
    }

    /// Set the currently focused room. This function als takes care of updating the breadcrumbs and active server
    ///
    /// Returns the new server if it changed
    pub async fn set_active_room(&self, room: Option<DePlaceRoom>) -> ActiveServer {
        tracing::trace!(
            "Setting active room: {:?}",
            room.as_ref().map(|r| r.room_id().to_owned())
        );

        let mut room_changed = false;
        let mut new_server = ActiveServer::Dms;

        self.inner.active_room.send_if_modified(|cur| {
            let changed = cur.as_ref().map(|r| r.room_id()) != room.as_ref().map(|r| r.room_id());
            if changed {
                *cur = room.clone();
                room_changed = true;
            }
            changed
        });

        if let Some(room) = room
            && room_changed
        {
            let room_id = room.room_id().to_owned();

            room.update_live_if_modified(|live| {
                live.notification_counts = NotificationCounts::default();
            });

            if let Some(event_id) = room.room.latest_event().event_id() {
                let sdk_room = room.room.clone();
                tokio::spawn(async move {
                    if let Err(e) = sdk_room
                        .send_single_receipt(ReceiptType::Read, ReceiptThread::Unthreaded, event_id)
                        .await
                    {
                        tracing::warn!("Failed to send read receipt: {e}");
                    }
                });
            }

            let active_server: ActiveServer = self.inner.room_store.get_server_of(&room_id).into();

            self.recalculate_title();

            new_server = active_server.clone();

            self.update_breadcrumbs(|breadcrumbs| {
                // Remove duplicates
                breadcrumbs.recent_rooms.retain(|id| id != &room_id);
                // Insert visited room at top
                breadcrumbs.recent_rooms.insert(0, room_id.clone());
                // Truncate to 25
                breadcrumbs.recent_rooms.truncate(25);

                match active_server {
                    ActiveServer::Dms => {
                        breadcrumbs.last_dm_id = Some(room_id);
                        breadcrumbs.dms_last = true;
                    }
                    ActiveServer::Server(room) => {
                        breadcrumbs
                            .last_space_ids
                            .insert(room.room_id().to_owned(), room_id);
                        breadcrumbs.dms_last = false;
                    }
                }
            });

            let client = self.client();
            let breadcrumbs = self.breadcrumbs();

            breadcrumbs.set(&client).await;
        }

        new_server
    }

    pub fn set_server_order(&self, server_order: Vec<OwnedRoomId>) {
        self.inner.server_order.send_if_modified(|cur| {
            if *cur != server_order {
                *cur = server_order;
                true
            } else {
                false
            }
        });
    }

    /// Returns the window title for the current state, taking active server and room into account.
    pub fn recalculate_title(&self) {
        if let Err(e) = self.inner.window_title.send(window_title(
            self.active_room().borrow().clone(),
            self.active_server().borrow().clone(),
        )) {
            tracing::error!("Failed to send window title: {}", e);
        }
    }
}

// Identity only, for Subscription diffing - not content equality.
// DO NOT use this for lazy/cache hashing.
impl Hash for AppState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (Arc::as_ptr(&self.inner) as usize).hash(state);
    }
}

pub fn to_stream<T: Clone, V>(
    rx: Receiver<T>,
    convert: impl Fn(T) -> V + Clone,
) -> impl Stream<Item = V> {
    stream::unfold(rx, move |mut rx| {
        let convert = convert.clone();
        async move {
            rx.changed().await.ok()?;
            let value = convert(rx.borrow().clone());
            Some((value, rx))
        }
    })
}
