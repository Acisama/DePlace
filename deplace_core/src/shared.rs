use std::{
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use arc_swap::ArcSwap;

pub struct Shared<T> {
    pub(crate) data: ArcSwap<T>,
    pub(crate) version: AtomicU64,
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Self {
            data: ArcSwap::from_pointee(T::default()),
            version: AtomicU64::new(0),
        }
    }
}

impl<T: Hash> Hash for Shared<T> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.data.load().hash(h);
    }
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

    pub fn read(&self) -> impl std::ops::Deref<Target = Arc<T>> {
        self.data.load()
    }

    pub fn set(&self, v: T) {
        self.data.store(Arc::new(v));
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn change(&self, f: impl Fn(&mut T)) {
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

impl<T: Clone + PartialEq> Shared<T> {
    pub fn change_if_modified(&self, f: impl Fn(&mut T)) {
        let mut changed = false;
        self.data.rcu(|cur| {
            let mut v = T::clone(cur);
            f(&mut v);
            changed = v != **cur;
            v
        });
        if changed {
            self.version.fetch_add(1, Ordering::Release);
        }
    }

    pub fn set_if_modified(&self, v: T) {
        if &v != self.data.load().as_ref() {
            self.set(v);
        }
    }
}
