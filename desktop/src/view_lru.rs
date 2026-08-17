use gpui::{AnyElement, Context, Focusable, IntoElement, Render, Window};
use std::hash::Hash;
use std::num::NonZeroUsize;

use gpui::Entity;
use lru::LruCache;

/// An LRU-bounded cache where at most one entry is "visible" at a time. Showing or
/// inserting an entry implicitly hides whatever was visible before, since visibility
/// is just a single current key rather than a flag on every entry.
pub struct VisibleLruCache<K: Hash + Eq + Clone, V> {
    cache: LruCache<K, Entity<V>>,
    visible: Option<K>,
    /// Used to render the empty state when no entry is visible.
    empty_render: fn() -> AnyElement,
}

impl<K: Hash + Eq + Clone + 'static, V: Focusable + 'static> VisibleLruCache<K, V> {
    pub fn new(cap: NonZeroUsize, empty_render: fn() -> AnyElement) -> Self {
        Self {
            cache: LruCache::new(cap),
            visible: None,
            empty_render,
        }
    }

    /// Inserts the value into the cache, makes it the visible entry, and focuses it -
    /// atomically, so there's no frame where the previous entry is still shown/focused.
    pub fn insert(
        &mut self,
        key: K,
        value: Entity<V>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&value.focus_handle(cx), cx);
        self.cache.push(key.clone(), value);
        self.visible = Some(key);
        cx.notify();
    }

    /// Makes `key` the visible entry and focuses it, if it's cached. Returns `false` if
    /// it isn't.
    pub fn show(&mut self, key: &K, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(entity) = self.cache.peek(key).cloned() {
            window.focus(&entity.focus_handle(cx), cx);
            self.cache.promote(key);
            self.visible = Some(key.clone());
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn hide_all(&mut self, cx: &mut Context<Self>) {
        self.visible = None;
        cx.notify();
    }

    // pub fn is_visible(&self, key: &K) -> bool {
    //     self.visible.as_ref() == Some(key)
    // }

    // pub fn visible_key(&self) -> Option<&K> {
    //     self.visible.as_ref()
    // }

    /// The entity currently visible, if any.
    pub fn visible(&self) -> Option<&Entity<V>> {
        self.visible.as_ref().and_then(|k| self.cache.peek(k))
    }

    // pub fn remove(&mut self, key: &K) {
    //     if self.is_visible(key) {
    //         self.visible = None;
    //     }
    //     self.cache.pop(key);
    // }
}

impl<K: Hash + Eq + Clone + 'static, V: Render> Render for VisibleLruCache<K, V> {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        if let Some(entity) = self.visible.as_ref().and_then(|v| self.cache.get_mut(v)) {
            entity.clone().into_any_element()
        } else {
            (self.empty_render)()
        }
    }
}
