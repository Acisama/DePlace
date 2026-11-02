use arc_swap::ArcSwap;
use profile::ProfileData;
use std::{
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use synth::SonicSignature;
use user_data::UserData;

use ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId, presence::PresenceState};

pub mod profile;
pub mod store;
pub mod synth;
pub mod user_data;

pub use store::ProfileStore;

pub struct Shared<T> {
    data: ArcSwap<T>,
    version: AtomicU64,
}

impl<T: std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.data.load().fmt(f)
    }
}

impl<T: Clone> Shared<T> {
    pub fn get(&self) -> Arc<T> {
        self.data.load_full()
    }

    pub fn get_ref(&self) -> impl std::ops::Deref<Target = Arc<T>> {
        self.data.load()
    }

    pub fn set(&self, v: T) {
        self.data.store(Arc::new(v));
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn modify(&self, f: impl Fn(&mut T)) {
        self.data.rcu(|cur| {
            let mut v = T::clone(cur);
            f(&mut v);
            v
        });
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn new(v: T) -> Self {
        Self {
            data: ArcSwap::from_pointee(v),
            version: AtomicU64::new(0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Member {
    user_id: OwnedUserId,
    room_id: OwnedRoomId,
    room: Arc<Shared<ProfileData>>,
    user: Arc<Shared<UserData>>,
}

impl Hash for Member {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.user_id.hash(h);
        self.room.version().hash(h);
        self.user.version().hash(h);
    }
}

impl PartialEq for Member {
    fn eq(&self, other: &Self) -> bool {
        self.user_id == other.user_id && self.room_id == other.room_id
    }
}

#[cfg(feature = "iced")]
use iced::advanced::image::Handle as ImageHandle;

use crate::colors::DePlaceColor;

impl Member {
    pub fn display_name(&self) -> String {
        self.room.get_ref().display_string.clone()
    }

    pub fn initial(&self) -> char {
        self.room.get_ref().initial
    }

    #[cfg(feature = "iced")]
    pub fn image_handle(&self) -> Option<Result<ImageHandle, Arc<String>>> {
        self.room.get_ref().avatar_handle.clone()
    }

    pub fn name_color(&self) -> DePlaceColor {
        self.user.get_ref().custom_data.name_color
    }

    pub fn banner_color(&self) -> DePlaceColor {
        self.user.get_ref().custom_data.banner_color
    }

    pub fn sonic_signature(&self) -> SonicSignature {
        self.user.get_ref().custom_data.sonic_signature.clone()
    }

    pub fn user_id(&self) -> &UserId {
        &self.user_id
    }

    pub fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    pub fn presence(&self) -> PresenceState {
        self.user.get_ref().presence.clone()
    }
}
