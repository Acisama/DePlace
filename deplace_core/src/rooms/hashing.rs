use std::{collections::BTreeSet, hash::Hash, sync::Arc};

use ruma::OwnedRoomId;

use super::{DePlaceRoom, DmRoomMap, GenericRoomMap, RoomWatchers};

#[derive(Debug, Clone)]
/// How to hash each object in a [`RoomWatchers`] map
pub enum HashingOption {
    /// Hash the room's ID
    Id,
    /// Hash the room's version (a monotonically increasing usize, will be different fire on every change of the room)
    Version,
    /// Hash the room's content (will be different when something from the filter actually changes)
    Room {
        config: RoomHashingConfig,
        /// If set, only rooms with an ID in this set will be hashed. Otherwise, all rooms will be hashed
        ids_to_check: Option<Arc<BTreeSet<OwnedRoomId>>>,
    },
}

/// Hashes the room's ID
pub fn hash_id() -> HashingOption {
    HashingOption::Id
}

/// Hashes the room's version
pub fn hash_version() -> HashingOption {
    HashingOption::Version
}

/// Hashes the room's content
pub fn hash_option_room(config: RoomHashingConfig) -> HashingOption {
    HashingOption::Room {
        config,
        ids_to_check: None,
    }
}

/// Hashes the room's content with the default configuration
pub fn hash_option_room_default() -> HashingOption {
    HashingOption::Room {
        config: RoomHashingConfig::default(),
        ids_to_check: None,
    }
}

/// Hashes the room's content with the default configuration and the given set of IDs to check
pub fn hash_option_room_with_ids(
    config: RoomHashingConfig,
    ids_to_check: Arc<BTreeSet<OwnedRoomId>>,
) -> HashingOption {
    HashingOption::Room {
        config,
        ids_to_check: Some(ids_to_check),
    }
}

/// Hashes the room's content with the default configuration and the given set of IDs to check
pub fn hash_option_room_with_ids_default(config: RoomHashingConfig) -> HashingOption {
    HashingOption::Room {
        config,
        ids_to_check: None,
    }
}

impl Default for HashingOption {
    fn default() -> Self {
        Self::Room {
            config: RoomHashingConfig::default(),
            ids_to_check: None,
        }
    }
}

/// Hash dms with the given option
pub fn hash_dms(option: HashingOption) -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_dms(option)
}

/// Hash dms with the default option
pub fn hash_dms_default() -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_dms(HashingOption::default())
}

/// Hash servers with the given option
pub fn hash_servers(option: HashingOption) -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_servers(option)
}

/// Hash servers with the default option
pub fn hash_servers_default() -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_servers(HashingOption::default())
}

/// Hash spaces with the given option
pub fn hash_spaces(option: HashingOption) -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_spaces(option)
}

/// Hash spaces with the default option
pub fn hash_spaces_default() -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_spaces(HashingOption::default())
}

/// Hash all rooms with the given option
pub fn hash_all_rooms(option: HashingOption) -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_all_rooms(option)
}

/// Hash all rooms with the default option
pub fn hash_all_rooms_default() -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_all_rooms(HashingOption::default())
}

/// Hash a room with the default option
pub fn hash_room_default(room_id: OwnedRoomId) -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_all_rooms(HashingOption::Room {
        config: RoomHashingConfig::default(),
        ids_to_check: Some(Arc::new(BTreeSet::from([room_id]))),
    })
}

/// Hash a room with the given option
pub fn hash_room(room_id: OwnedRoomId, config: RoomHashingConfig) -> RoomWatcherHashingConfig {
    RoomWatcherHashingConfig::default().hash_all_rooms(HashingOption::Room {
        config,
        ids_to_check: Some(Arc::new(BTreeSet::from([room_id]))),
    })
}

#[derive(Debug, Clone, Default)]
/// Configuration for hashing [`RoomWatchers`]
pub struct RoomWatcherHashingConfig {
    /// Whether to hash DMs
    hash_dms: Option<HashingOption>,
    /// Whether to hash servers
    hash_servers: Option<HashingOption>,
    /// Whether to hash spaces
    hash_spaces: Option<HashingOption>,
    /// Whether to hash all rooms
    hash_all_rooms: Option<HashingOption>,
}

impl RoomWatcherHashingConfig {
    pub fn hash_dms(&mut self, hash_dms: HashingOption) -> Self {
        self.hash_dms = Some(hash_dms);
        self.clone()
    }

    pub fn hash_servers_default(&mut self) -> Self {
        self.hash_servers = Some(HashingOption::default());
        self.clone()
    }

    pub fn hash_servers(&mut self, hash_servers: HashingOption) -> Self {
        self.hash_servers = Some(hash_servers);
        self.clone()
    }

    pub fn hash_spaces_default(&mut self) -> Self {
        self.hash_spaces = Some(HashingOption::default());
        self.clone()
    }

    pub fn hash_spaces(&mut self, hash_spaces: HashingOption) -> Self {
        self.hash_spaces = Some(hash_spaces);
        self.clone()
    }

    pub fn hash_all_rooms_default(&mut self) -> Self {
        self.hash_all_rooms = Some(HashingOption::default());
        self.clone()
    }

    pub fn hash_all_rooms(&mut self, hash_all_rooms: HashingOption) -> Self {
        self.hash_all_rooms = Some(hash_all_rooms);
        self.clone()
    }
}

impl Hash for RoomWatchers {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if let Some(hash_all_rooms) = &self.hash_config.hash_all_rooms {
            self.all_rooms.borrow().hash(hash_all_rooms, state);
        }

        if let Some(hash_dms) = &self.hash_config.hash_dms {
            self.dm_rooms.borrow().hash(hash_dms, state);
        }

        if let Some(hash_servers) = &self.hash_config.hash_servers {
            self.other_rooms.borrow().hash(hash_servers, state);
        }

        if let Some(hash_spaces) = &self.hash_config.hash_spaces {
            self.spaces.borrow().hash(hash_spaces, state);
        }
    }
}

impl GenericRoomMap {
    fn hash<H: std::hash::Hasher>(&self, option: &HashingOption, state: &mut H) {
        match option {
            HashingOption::Id => self
                .get_all_rooms()
                .iter()
                .for_each(|r| r.room_id().hash(state)),
            HashingOption::Version => {
                self.inner.versions.len().hash(state);
                self.inner.versions.iter().for_each(|v| v.hash(state));
                self.inner.total_version.hash(state);
            }
            HashingOption::Room {
                config,
                ids_to_check,
            } => self.get_all_rooms().iter().for_each(|r| {
                if let Some(ids_to_check) = &ids_to_check
                    && !ids_to_check.contains(r.room_id())
                {
                    return;
                }
                r.hash(config, state)
            }),
        }
    }
}

impl DmRoomMap {
    fn hash<H: std::hash::Hasher>(&self, option: &HashingOption, state: &mut H) {
        match option {
            HashingOption::Id => self.inner.vec.iter().for_each(|r| r.room_id().hash(state)),
            HashingOption::Version => {
                self.inner.versions.len().hash(state);
                self.inner.versions.iter().for_each(|v| v.hash(state));
                self.inner.total_version.hash(state);
            }
            HashingOption::Room {
                config,
                ids_to_check,
            } => self.inner.vec.iter().for_each(|r| {
                if let Some(ids_to_check) = &ids_to_check
                    && !ids_to_check.contains(r.room_id())
                {
                    return;
                }
                r.hash(config, state)
            }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// Which fields of a [`DePlaceRoom`] should be hashed
pub struct RoomHashingConfig {
    /// Whether the `display_name` should be hashed
    hash_display_name: bool,
    /// Whether the `avatar_url` should be hashed
    hash_avatar_url: bool,
    /// Whether the `notification_counts` should be hashed
    hash_notifications: bool,
}

impl Default for RoomHashingConfig {
    fn default() -> Self {
        Self {
            hash_display_name: true,
            hash_avatar_url: true,
            hash_notifications: false,
        }
    }
}

impl RoomHashingConfig {
    pub fn hash_display_name(&mut self) -> Self {
        self.hash_display_name = true;
        *self
    }

    pub fn hash_avatar_url(&mut self) -> Self {
        self.hash_avatar_url = true;
        *self
    }

    pub fn hash_notifications(&mut self, hash_notifications: bool) -> Self {
        self.hash_notifications = hash_notifications;
        *self
    }
}

/// Hash `display_name`
pub fn hash_display_name() -> RoomHashingConfig {
    RoomHashingConfig::default().hash_display_name()
}

/// Hash `avatar_url`
pub fn hash_avatar_url() -> RoomHashingConfig {
    RoomHashingConfig::default().hash_avatar_url()
}

/// Hash `notification_counts`
pub fn hash_notifications() -> RoomHashingConfig {
    RoomHashingConfig::default().hash_notifications(true)
}

/// Hash `display_name` and `avatar_url`
pub fn hash_visible() -> RoomHashingConfig {
    RoomHashingConfig::default()
        .hash_display_name()
        .hash_avatar_url()
}

impl DePlaceRoom {
    pub fn hash<H: std::hash::Hasher>(&self, config: &RoomHashingConfig, state: &mut H) {
        if config.hash_display_name {
            self.inner.display_name.hash(state);
        }
        if config.hash_avatar_url {
            self.inner.avatar_url.hash(state);
        }
        if config.hash_notifications {
            self.inner.notification_counts.hash(state);
        }
    }
}
