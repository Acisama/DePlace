use std::{collections::HashMap, sync::Arc};

use crate::components::message::CachedTimelineItem;
use chrono::{DateTime, Utc};
use dashmap::{DashMap, mapref::one::RefMut};
use gpui::{ListOffset, SharedString};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId};
use uuid::Uuid;

pub struct Attachment {
    pub name: String,
    pub mime_type: String,
    pub size: Option<usize>,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct SearchParameters {
    pub search_id: Uuid,
    pub room_ids: Vec<OwnedRoomId>,
    pub text: String,
    pub senders: Vec<OwnedUserId>,
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
    pub has_link: bool,
}

#[derive(Default, Clone)]
pub struct RoomState {
    pub scroll_offset: ListOffset,
    pub chat_input: SharedString,
    pub attachments: Arc<Vec<Attachment>>,
    pub search_parameters: Option<SearchParameters>,
    pub search_results: Option<HashMap<OwnedRoomId, Vec<CachedTimelineItem>>>,
    pub pinned_result: Option<Vec<CachedTimelineItem>>,
}

/// A mutable handle to one room's state. Derefs to [`RoomState`], so fields can be
/// mutated directly, e.g. `store.entry(id).chat_input = "...".into()`.
pub type RoomStateRef<'a> = RefMut<'a, OwnedRoomId, RoomState>;

#[derive(Clone)]
pub struct RoomStateStore(Arc<DashMap<OwnedRoomId, RoomState>>);

impl RoomStateStore {
    pub fn new() -> Self {
        Self(Arc::new(DashMap::new()))
    }

    pub fn entry(&self, room_id: &RoomId) -> RoomStateRef<'_> {
        self.0.entry(room_id.to_owned()).or_default()
    }

    pub fn get(&self, room_id: &RoomId) -> Option<RoomState> {
        self.0.get(room_id).map(|e| e.clone())
    }
}
