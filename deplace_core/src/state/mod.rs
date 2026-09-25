use std::{
    collections::{BTreeSet, HashMap},
    env::temp_dir,
    hash::Hash,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use futures::{Stream, stream};
use matrix_sdk::{
    Client,
    room::RoomMember,
    ruma::{OwnedDeviceId, OwnedRoomId, OwnedUserId},
};
use ruma::{RoomId, events::presence::PresenceEventContent};
use tokio::sync::watch::{self, Receiver, Ref, Sender};

use crate::{
    APP_NAME,
    keybinds::Keybinds,
    matrix_api::account_data::{
        BreadcrumbsContent, ServerOrderContent, get_account_data, set_account_data,
    },
    notifications::NotificationManager,
    rooms::{DePlaceRoom, RoomWatcherHashingConfig, RoomWatchers, SpaceHierarchy},
    settings::Settings,
    window_title,
};

#[derive(Debug, Clone)]
pub struct ImportantPaths {
    pub config_dir: PathBuf,
    pub settings_file: PathBuf,
    pub keybinds_file: PathBuf,
    pub theme_file: PathBuf,
    pub structure_file: PathBuf,

    pub download_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub temp_dir: PathBuf,
    pub data_dir: PathBuf,
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

        if !config_dir.exists() {
            std::fs::create_dir_all(&config_dir)?;
        }
        if !download_dir.exists() {
            std::fs::create_dir_all(&download_dir)?;
        }
        if !cache_dir.exists() {
            std::fs::create_dir_all(&cache_dir)?;
        }
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)?;
        }

        let settings_file = config_dir.join("config.toml");
        let keybinds_file = config_dir.join("keybinds.toml");
        let theme_file = config_dir.join("theme.toml");
        let structure_file = config_dir.join("structure.toml");

        Ok(Self {
            config_dir,
            settings_file,
            keybinds_file,
            theme_file,
            structure_file,

            download_dir,
            cache_dir,
            temp_dir,
            data_dir,
        })
    }
}

#[derive(Clone, Debug)]
pub struct UserDevice {
    pub user_id: OwnedUserId,
    pub device_id: OwnedDeviceId,
}

pub type MembershipMap = HashMap<OwnedRoomId, HashMap<OwnedUserId, RoomMember>>;
pub type PresenceMap = HashMap<OwnedUserId, PresenceEventContent>;

pub mod cache;
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

/// Cheaply clonable AppState since the data is all
/// wrapped in an `Arc`. Access only over functions,
/// no direct field access.
#[derive(Clone, Debug)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

#[derive(Debug)]
struct AppStateInner {
    pub client: Client,
    pub own_device: Arc<UserDevice>,
    pub settings: Settings,

    keybinds: Sender<Keybinds>,
    room_watchers: RoomWatchers,

    window_title: Sender<String>,
    window_focused: Mutex<bool>,
    important_paths: ImportantPaths,

    notification_manager: NotificationManager,

    pub membership_version: Sender<u64>,
    pub presence_version: Sender<u64>,
    sync_tick: Sender<u64>,

    pub active_room: Sender<Option<DePlaceRoom>>,
    pub active_server: Sender<ActiveServer>,
    presence_map: Sender<PresenceMap>,
    membership_map: Sender<MembershipMap>,

    server_order: Sender<Vec<OwnedRoomId>>,

    /// Last accessed servers and rooms, since the data is only
    /// needed as snapshots, it is held in a mutex
    breadcrumbs: Mutex<BreadcrumbsContent>,

    #[cfg(feature = "iced_desktop")]
    avatar_cache: cache::AvatarCache,

    #[cfg(feature = "iced_desktop")]
    thumbnail_cache: cache::ThumbnailCache,

    #[cfg(feature = "iced_desktop")]
    video_cache: cache::VideoCache,
}

impl AppState {
    pub async fn new(
        client: Client,
        user_device: UserDevice,
        settings: Settings,
        keybinds: Keybinds,
        important_paths: ImportantPaths,
    ) -> Result<Self> {
        let breadcrumbs_content = get_account_data::<BreadcrumbsContent>(&client).await;
        let last_server_order = get_account_data(&client).await;

        let own_device = Arc::new(user_device);

        let room_watchers =
            RoomWatchers::new(client.clone(), last_server_order, own_device.clone()).await?;
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
            && let Some(server) = room_watchers.get_server_of(room_id)
        {
            ActiveServer::Server(server)
        } else {
            ActiveServer::Dms
        };

        let active_room = if let Some(room) = last_room_id.and_then(|id| client.get_room(&id)) {
            Some(DePlaceRoom::from_room(room, &own_device).await)
        } else {
            None
        };

        let (active_room, _) = watch::channel(active_room);
        let (active_server, _) = watch::channel(active_server);

        let (membership_map, _) = watch::channel(MembershipMap::default());
        let (presence_map, _) = watch::channel(PresenceMap::default());

        let server_order_data = get_account_data::<ServerOrderContent>(&client).await;
        let (server_order, _) = watch::channel(server_order_data.servers);

        let (membership_version, _) = watch::channel(0);
        let (presence_version, _) = watch::channel(0);
        let (sync_tick, _) = watch::channel(0);

        let (window_title, _) = watch::channel(window_title(
            active_room.borrow().clone(),
            active_server.borrow().clone(),
        ));

        let (keybinds, _) = watch::channel(keybinds);

        Ok(Self {
            inner: Arc::new(AppStateInner {
                #[cfg(feature = "iced_desktop")]
                avatar_cache: cache::AvatarCache::new(client.clone()),

                #[cfg(feature = "iced_desktop")]
                thumbnail_cache: cache::ThumbnailCache::new(client.clone()),

                #[cfg(feature = "iced_desktop")]
                video_cache: cache::VideoCache::new(client.clone()),

                room_watchers,

                window_title,
                window_focused: Mutex::new(false),
                important_paths,

                notification_manager: NotificationManager::default(),

                client,
                own_device,
                settings,
                keybinds,

                membership_version,
                presence_version,
                sync_tick,

                active_room,
                active_server,

                membership_map,
                presence_map,

                server_order,
                breadcrumbs,
            }),
        })
    }

    #[cfg(feature = "iced_desktop")]
    pub fn avatar_cache(&self) -> &cache::AvatarCache {
        &self.inner.avatar_cache
    }

    #[cfg(feature = "iced_desktop")]
    pub fn thumbnail_cache(&self) -> &cache::ThumbnailCache {
        &self.inner.thumbnail_cache
    }

    #[cfg(feature = "iced_desktop")]
    pub fn video_cache(&self) -> &cache::VideoCache {
        &self.inner.video_cache
    }

    fn inner_room_watchers(&self) -> &RoomWatchers {
        &self.inner.room_watchers
    }

    pub fn room_watchers(&self, config: RoomWatcherHashingConfig) -> RoomWatchers {
        let mut watchers = self.inner.room_watchers.clone();
        watchers.hash_config = config;
        watchers
    }

    pub fn get_children(&self, parent: &RoomId) -> Arc<Vec<OwnedRoomId>> {
        self.hierarchy().get_children(parent)
    }

    pub fn get_all_children(&self, parent: &RoomId) -> Arc<BTreeSet<OwnedRoomId>> {
        self.hierarchy().get_all_children(parent)
    }

    pub fn hierarchy(&self) -> Ref<'_, SpaceHierarchy> {
        self.inner.room_watchers.hierarchy.borrow()
    }

    pub fn servers(&self) -> Arc<Vec<OwnedRoomId>> {
        self.hierarchy().servers.clone()
    }

    pub fn get_room(&self, room_id: &RoomId) -> Option<DePlaceRoom> {
        self.inner_room_watchers().all_rooms().get(room_id)
    }

    pub fn keybinds(&self) -> Receiver<Keybinds> {
        self.inner.keybinds.subscribe()
    }

    pub fn notification_manager(&self) -> &NotificationManager {
        &self.inner.notification_manager
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
        tracing::trace!("Window focused: focused={focused}");
        *self
            .inner
            .window_focused
            .lock()
            .unwrap_or_else(|posion| posion.into_inner()) = focused;
    }

    pub fn window_title(&self) -> Receiver<String> {
        self.inner.window_title.subscribe()
    }

    pub fn membership_version(&self) -> Ref<'_, u64> {
        self.inner.membership_version.borrow()
    }

    pub fn presence_version(&self) -> Ref<'_, u64> {
        self.inner.presence_version.borrow()
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

    pub fn membership_map(&self) -> watch::Receiver<MembershipMap> {
        self.inner.membership_map.subscribe()
    }

    pub fn presence_map(&self) -> watch::Receiver<PresenceMap> {
        self.inner.presence_map.subscribe()
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

    pub(crate) fn set_membership_map(&self, membership_map: MembershipMap) {
        self.inner.membership_version.send_modify(|v| *v += 1);
        self.inner.membership_map.send_if_modified(|cur| {
            *cur = membership_map;
            true
        });
    }

    pub(crate) fn add_membership(&self, room_id: OwnedRoomId, member: RoomMember) {
        self.inner.membership_version.send_modify(|v| *v += 1);
        self.inner.membership_map.send_if_modified(|cur| {
            cur.entry(room_id)
                .or_default()
                .insert(member.user_id().to_owned(), member);
            true
        });
    }

    pub(crate) fn bump_sync_tick(&self) {
        self.inner.sync_tick.send_modify(|v| *v = v.wrapping_add(1));
    }

    pub(crate) fn add_presences(&self, presences: PresenceMap) {
        self.inner.presence_version.send_modify(|v| *v += 1);
        self.inner.presence_map.send_if_modified(|cur| {
            cur.extend(presences);
            true
        });
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

            let new_room_id = match server_id {
                ActiveServerId::Server(id) => {
                    breadcrumbs.last_space_ids.get(&id).cloned().or_else(|| {
                        self.hierarchy()
                            .parent_to_children
                            .get(&id)
                            .and_then(|children| children.first().cloned())
                    })
                }
                ActiveServerId::Dms => breadcrumbs.last_dm_id.clone().or_else(|| {
                    self.inner
                        .room_watchers
                        .dm_rooms()
                        .iter()
                        .next()
                        .map(|r| r.room_id().to_owned())
                }),
            };

            if change_room {
                let new_room = new_room_id.and_then(|id| self.inner.room_watchers.get_room(&id));
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

            let active_server_id = self.hierarchy().get_server_of(&room_id);

            let active_server: ActiveServer =
                active_server_id.and_then(|id| self.get_room(&id)).into();

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
                    }
                }
            });

            let client = self.client();
            let breadcrumbs = self.breadcrumbs();

            set_account_data(&client, breadcrumbs).await;
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
