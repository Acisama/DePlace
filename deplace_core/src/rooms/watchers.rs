use anyhow::Result;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use futures::StreamExt;
use indexmap::IndexMap;
use matrix_sdk::Client;
use matrix_sdk_ui::{
    RoomListService,
    eyeball_im::{Vector, VectorDiff},
    room_list_service::{
        RoomList, RoomListItem,
        filters::{self, BoxedFilterFn, RoomCategory},
    },
};
use ruma::{OwnedRoomId, RoomId};
use tokio::sync::watch;

use super::{DePlaceRoom, RoomWatcherHashingConfig, SpaceHierarchy};

#[derive(Debug, Clone)]
pub struct RoomWatchers {
    pub(crate) dm_rooms: watch::Receiver<DmRoomMap>,
    pub(crate) spaces: watch::Receiver<GenericRoomMap>,
    pub(crate) other_rooms: watch::Receiver<GenericRoomMap>,
    pub(crate) all_rooms: watch::Receiver<GenericRoomMap>,

    pub hierarchy: watch::Receiver<SpaceHierarchy>,

    pub(crate) hash_config: RoomWatcherHashingConfig,

    _service: Arc<RoomListService>,
    _room_list: Arc<RoomList>,
}

impl RoomWatchers {
    pub async fn new(client: Client) -> Result<Self> {
        let service = RoomListService::new(client.clone()).await?;

        let room_list = Arc::new(service.all_rooms().await?);

        let new_filter = move |filter| {
            Box::new(filters::new_filter_all(vec![
                Box::new(filters::new_filter_non_left()),
                filter,
            ]))
        };

        let dm_filter = || Box::new(filters::new_filter_category(RoomCategory::People));
        let space_filter = || Box::new(filters::new_filter_space());

        let (hier_tx, mut hier_rx) = tokio::sync::mpsc::unbounded_channel();
        let (hier_watch_tx, hier_watch_rx) = watch::channel(SpaceHierarchy::default());

        tokio::spawn(async move {
            let mut hierarchy = SpaceHierarchy::default();

            // Listen for incoming parent updates from the stream updaters
            while let Some((child_id, parents)) = hier_rx.recv().await {
                let old_version = hierarchy.total_version;

                hierarchy.update_child_parents(child_id, parents);

                // Only broadcast to the UI if something actually changed
                if hierarchy.total_version != old_version
                    && hier_watch_tx.send(hierarchy.clone()).is_err()
                {
                    break; // UI dropped the receiver, shut down task
                }
            }
        });

        Ok(RoomWatchers {
            dm_rooms: spawn_generic_updater(
                client.clone(),
                room_list.clone(),
                new_filter(dm_filter()),
                hier_tx.clone(),
            )
            .await,
            spaces: spawn_generic_updater(
                client.clone(),
                room_list.clone(),
                new_filter(space_filter()),
                hier_tx.clone(),
            )
            .await,
            other_rooms: spawn_generic_updater(
                client.clone(),
                room_list.clone(),
                new_filter(Box::new(filters::new_filter_not(Box::new(
                    filters::new_filter_any(vec![dm_filter(), space_filter()]),
                )))),
                hier_tx.clone(),
            )
            .await,
            all_rooms: spawn_generic_updater(
                client.clone(),
                room_list.clone(),
                Box::new(filters::new_filter_not(
                    Box::new(filters::new_filter_none()),
                )),
                hier_tx.clone(),
            )
            .await,

            hierarchy: hier_watch_rx,

            hash_config: RoomWatcherHashingConfig::default(),

            _room_list: room_list,
            _service: Arc::new(service),
        })
    }

    pub fn get_children(&self, parent: &RoomId) -> Vec<DePlaceRoom> {
        let all_rooms = self.all_rooms.borrow();
        self.hierarchy
            .borrow()
            .get_children(parent)
            .iter()
            .filter_map(|id| all_rooms.get(id))
            .collect()
    }

    pub fn get_all_children(&self, parent: &RoomId) -> Vec<DePlaceRoom> {
        let all_rooms = self.all_rooms.borrow();
        self.hierarchy
            .borrow()
            .get_all_children(parent)
            .iter()
            .filter_map(|id| all_rooms.get(id))
            .collect()
    }

    pub fn dm_rooms(&self) -> Arc<Vec<DePlaceRoom>> {
        self.dm_rooms.borrow().as_vec()
    }

    pub fn servers(&self) -> Vec<DePlaceRoom> {
        let all_rooms = self.all_rooms.borrow();
        self.hierarchy
            .borrow()
            .servers
            .iter()
            .filter_map(|id| all_rooms.get(id))
            .collect()
    }

    pub fn all_rooms(&self) -> GenericRoomMap {
        let all_rooms = self.all_rooms.borrow();
        all_rooms.clone()
    }

    pub fn get_room(&self, room_id: &RoomId) -> Option<DePlaceRoom> {
        self.all_rooms.borrow().get(room_id)
    }

    pub fn get_server_of(&self, room_id: &RoomId) -> Option<DePlaceRoom> {
        let server_id = self.hierarchy.borrow().get_server_of(room_id)?;
        self.all_rooms.borrow().get(&server_id)
    }
}

async fn fetch_and_notify_room(
    client: &Client,
    item: &RoomListItem,
    hier_tx: &tokio::sync::mpsc::UnboundedSender<(
        OwnedRoomId,
        HashMap<OwnedRoomId, Option<String>>,
    )>,
) -> DePlaceRoom {
    let room = client
        .get_room(item.room_id())
        .expect("Room missing from client");

    let dp_room = DePlaceRoom::from_room(room).await;

    if let Err(e) = hier_tx.send((dp_room.room_id().to_owned(), dp_room.parents())) {
        tracing::error!("Failed to send room hierarchy: {}", e);
    }

    dp_room
}

async fn spawn_generic_updater<T: UpdateExt + Default + Send + Sync + 'static + Clone>(
    client: Client,
    room_list: Arc<matrix_sdk_ui::room_list_service::RoomList>,
    filter: BoxedFilterFn,
    hier_tx: tokio::sync::mpsc::UnboundedSender<(
        OwnedRoomId,
        HashMap<OwnedRoomId, Option<String>>,
    )>,
) -> watch::Receiver<T> {
    let (tx, rx) = watch::channel(T::default());

    tokio::spawn(async move {
        let mut current_map = T::default();
        let (stream, config) = room_list.entries_with_dynamic_adapters(50);

        tokio::pin!(stream);

        config.set_filter(filter);

        while let Some(diffs) = stream.next().await {
            let mut mapped_diffs = Vec::with_capacity(diffs.len());

            for diff in diffs {
                let mapped = match diff {
                    VectorDiff::Append { values } => {
                        let mut new_vals: Vector<_> = Vec::with_capacity(values.len()).into();
                        for v in values {
                            new_vals.push_back(fetch_and_notify_room(&client, &v, &hier_tx).await);
                        }
                        VectorDiff::Append { values: new_vals }
                    }
                    VectorDiff::Clear => VectorDiff::Clear,
                    VectorDiff::Insert { index, value } => VectorDiff::Insert {
                        index,
                        value: fetch_and_notify_room(&client, &value, &hier_tx).await,
                    },
                    VectorDiff::PopBack => VectorDiff::PopBack,
                    VectorDiff::PopFront => VectorDiff::PopFront,
                    VectorDiff::PushBack { value } => VectorDiff::PushBack {
                        value: fetch_and_notify_room(&client, &value, &hier_tx).await,
                    },
                    VectorDiff::PushFront { value } => VectorDiff::PushFront {
                        value: fetch_and_notify_room(&client, &value, &hier_tx).await,
                    },
                    VectorDiff::Remove { index } => VectorDiff::Remove { index },
                    VectorDiff::Reset { values } => {
                        let mut new_vals: Vector<_> = Vec::with_capacity(values.len()).into();
                        for v in values {
                            new_vals.push_back(fetch_and_notify_room(&client, &v, &hier_tx).await);
                        }
                        VectorDiff::Reset { values: new_vals }
                    }
                    VectorDiff::Set { index, value } => VectorDiff::Set {
                        index,
                        value: fetch_and_notify_room(&client, &value, &hier_tx).await,
                    },
                    VectorDiff::Truncate { length } => VectorDiff::Truncate { length },
                };

                mapped_diffs.push(mapped);
            }

            current_map.update(mapped_diffs);
            if tx.send(current_map.clone()).is_err() {
                break; // Receiver was dropped, exit task
            }
        }
    });

    rx
}

trait UpdateExt {
    fn update(&mut self, diffs: Vec<VectorDiff<DePlaceRoom>>);
}

#[derive(Default, Clone, Debug)]
pub struct DmRoomMap {
    pub(crate) inner: Arc<DmRoomMapInner>,
}

#[derive(Default, Clone, Debug)]
pub(crate) struct DmRoomMapInner {
    pub vec: Arc<Vec<DePlaceRoom>>,
    pub versions: Vec<u64>,
    pub total_version: u64,
}

impl DmRoomMap {
    pub fn as_vec(&self) -> Arc<Vec<DePlaceRoom>> {
        self.inner.vec.clone()
    }
}

impl UpdateExt for DmRoomMap {
    fn update(&mut self, diffs: Vec<VectorDiff<DePlaceRoom>>) {
        let inner = Arc::make_mut(&mut self.inner);

        let versions = &mut inner.versions;
        let vec = Arc::make_mut(&mut inner.vec);

        for diff in diffs {
            match diff {
                VectorDiff::Append { values } => {
                    versions.extend(std::iter::repeat_n(0, values.len()));
                    vec.extend(values);
                }
                VectorDiff::Clear => {
                    vec.clear();
                    versions.clear();
                }
                VectorDiff::Insert { index, value } => {
                    vec.insert(index, value);
                    versions.insert(index, 0);
                }
                VectorDiff::PopBack => {
                    vec.pop();
                    versions.pop();
                }
                VectorDiff::PopFront => {
                    vec.remove(0);
                    versions.remove(0);
                }
                VectorDiff::PushBack { value } => {
                    vec.push(value);
                    versions.push(0);
                }
                VectorDiff::PushFront { value } => {
                    vec.insert(0, value);
                    versions.insert(0, 0);
                }
                VectorDiff::Remove { index } => {
                    vec.remove(index);
                    versions.remove(index);
                }
                VectorDiff::Reset { values } => {
                    *versions = values.iter().map(|_| 0).collect();
                    *vec = values.into_iter().collect();
                    inner.total_version += 1;
                }
                VectorDiff::Set { index, value } => {
                    vec[index] = value;
                    versions[index] += 1;
                }
                VectorDiff::Truncate { length } => {
                    vec.truncate(length);
                    versions.truncate(length);
                }
            }
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct GenericRoomMap {
    pub(crate) inner: Arc<GenericRoomMapInner>,
}

impl GenericRoomMap {
    pub fn get(&self, room_id: &RoomId) -> Option<DePlaceRoom> {
        self.inner.map.get(room_id).cloned()
    }

    pub fn get_all_rooms(&self) -> Vec<DePlaceRoom> {
        self.inner.map.values().cloned().collect()
    }
}

#[derive(Default, Clone, Debug)]
pub(crate) struct GenericRoomMapInner {
    pub map: IndexMap<OwnedRoomId, DePlaceRoom>,
    pub versions: BTreeMap<OwnedRoomId, u64>,
    pub total_version: u64,
}

impl UpdateExt for GenericRoomMap {
    fn update(&mut self, diffs: Vec<VectorDiff<DePlaceRoom>>) {
        let diffs = diffs
            .into_iter()
            .map(|diff| diff.map(|room| (room.room_id().to_owned(), room)));

        let inner = Arc::make_mut(&mut self.inner);

        let versions = &mut inner.versions;
        let map = &mut inner.map;

        for diff in diffs {
            match diff {
                VectorDiff::Append { values } => {
                    versions.extend(values.iter().map(|(id, _)| (id.clone(), 0)));
                    map.extend(values);
                }
                VectorDiff::Clear => {
                    map.clear();
                    versions.clear();
                }
                VectorDiff::Insert {
                    index,
                    value: (key, value),
                } => {
                    map.shift_insert(index, key.clone(), value);
                    versions.insert(key, 0);
                }
                VectorDiff::PopBack => {
                    let Some((key, _)) = map.pop() else {
                        return;
                    };
                    versions.remove(&key);
                }
                VectorDiff::PopFront => {
                    let Some((key, _)) = map.swap_remove_index(0) else {
                        return;
                    };
                    versions.remove(&key);
                }
                VectorDiff::PushBack {
                    value: (key, value),
                } => {
                    map.insert(key.clone(), value);
                    versions.insert(key, 0);
                }
                VectorDiff::PushFront {
                    value: (key, value),
                } => {
                    map.shift_insert(0, key.clone(), value);
                    versions.insert(key, 0);
                }
                VectorDiff::Remove { index } => {
                    let Some((key, _)) = map.swap_remove_index(index) else {
                        return;
                    };
                    versions.remove(&key);
                }
                VectorDiff::Reset { values } => {
                    *versions = values.iter().map(|(key, _)| (key.clone(), 0)).collect();
                    inner.total_version += 1;
                    *map = values.into_iter().collect();
                }
                VectorDiff::Set {
                    index,
                    value: (key, value),
                } => {
                    map.shift_insert(index, key.clone(), value);
                    *versions.entry(key).or_insert(0) += 1;
                }
                VectorDiff::Truncate { length } => {
                    map.truncate(length);
                    *versions = versions
                        .iter()
                        .take(length)
                        .map(|(k, _)| (k.clone(), 0))
                        .collect();
                }
            }
        }
    }
}
