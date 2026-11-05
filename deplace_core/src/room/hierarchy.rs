use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use ruma::{MilliSecondsSinceUnixEpoch, OwnedRoomId, RoomId};

use crate::matrix_api::account_data::ServerOrderContent;

/// Fields needed to order a child room within a space, mirroring the
/// `m.space.child` ordering algorithm: rooms with a valid `order` sort
/// lexicographically ahead of those without one; ties (including all
/// order-less rooms) fall back to `origin_server_ts`, then room ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildOrder {
    pub order: Option<String>,
    pub origin_server_ts: Option<MilliSecondsSinceUnixEpoch>,
}

#[derive(Clone, Debug)]
pub struct SpaceHierarchy {
    pub parent_to_children: Arc<BTreeMap<OwnedRoomId, Arc<Vec<OwnedRoomId>>>>,
    pub parent_to_all_children: Arc<BTreeMap<OwnedRoomId, Arc<BTreeSet<OwnedRoomId>>>>,
    pub child_to_parents: Arc<BTreeMap<OwnedRoomId, BTreeSet<OwnedRoomId>>>,

    pub parent_to_child_orders: Arc<BTreeMap<OwnedRoomId, BTreeMap<OwnedRoomId, ChildOrder>>>,

    pub servers: Arc<Vec<OwnedRoomId>>,
    last_server_order: Arc<ServerOrderContent>,

    pub parent_versions: Arc<BTreeMap<OwnedRoomId, u64>>,
    pub total_version: u64,
}

impl SpaceHierarchy {
    pub fn new(last_server_order: ServerOrderContent) -> Self {
        Self {
            parent_to_children: Arc::new(BTreeMap::new()),
            parent_to_all_children: Arc::new(BTreeMap::new()),
            child_to_parents: Arc::new(BTreeMap::new()),

            parent_to_child_orders: Arc::new(BTreeMap::new()),

            servers: Arc::new(last_server_order.servers.clone()),
            last_server_order: Arc::new(last_server_order),

            parent_versions: Arc::new(BTreeMap::new()),
            total_version: 0,
        }
    }

    fn recompute_all_children(&mut self) {
        let mut new_all_children = BTreeMap::new();
        let mut new_children_to_parents: BTreeMap<OwnedRoomId, BTreeSet<OwnedRoomId>> =
            BTreeMap::new();

        for parent_id in self.parent_to_children.keys() {
            let mut all_kids = BTreeSet::new();
            let mut stack = vec![parent_id.clone()];
            let mut visited = HashSet::new(); // Prevent infinite loops from cyclic spaces

            while let Some(current_node) = stack.pop() {
                if !visited.insert(current_node.clone()) {
                    continue;
                }

                if let Some(children) = self.parent_to_children.get(&current_node) {
                    for child in children.iter() {
                        all_kids.insert(child.clone());
                        stack.push(child.clone());
                        if let Some(parents) = new_children_to_parents.get_mut(child) {
                            parents.insert(parent_id.clone());
                        } else {
                            new_children_to_parents
                                .insert(child.clone(), BTreeSet::from([parent_id.clone()]));
                        }
                    }
                }
            }

            new_all_children.insert(parent_id.clone(), Arc::new(all_kids.into_iter().collect()));
        }

        self.servers = Arc::new(
            self.parent_to_children
                .keys()
                .filter(|id| !new_children_to_parents.contains_key(*id))
                .cloned()
                .collect(),
        );
        self.parent_to_all_children = Arc::new(new_all_children);
        self.child_to_parents = Arc::new(new_children_to_parents);
    }

    fn sort_children(&mut self, parent: &RoomId) {
        if let Some(children) = Arc::make_mut(&mut self.parent_to_children).get_mut(parent) {
            let orders = self.parent_to_child_orders.get(parent);
            Arc::make_mut(children).sort_by_key(|id| {
                let child_order = orders.and_then(|map| map.get(id));
                let order = child_order.and_then(|c| c.order.clone());
                let no_order = order.is_none();
                let ts = child_order.and_then(|c| c.origin_server_ts);
                (no_order, order.unwrap_or_default(), ts, id.clone())
            });
        }
    }

    fn apply_server_order(&mut self) {
        let current_servers: BTreeSet<OwnedRoomId> = self.servers.iter().cloned().collect();

        let known: BTreeSet<&OwnedRoomId> = self.last_server_order.servers.iter().collect();
        let mut remembered = self.last_server_order.servers.clone();
        for server in &current_servers {
            if !known.contains(server) {
                remembered.push(server.clone());
            }
        }
        if remembered != self.last_server_order.servers {
            self.last_server_order = Arc::new(ServerOrderContent {
                servers: remembered,
            });
        }

        let mut remaining = current_servers;
        let mut new_server_order = Vec::new();
        for server in &self.last_server_order.servers {
            if remaining.remove(server) {
                new_server_order.push(server.clone());
            }
        }
        new_server_order.extend(remaining);

        if *self.servers != new_server_order {
            self.servers = Arc::new(new_server_order);
        }
    }

    fn bump_parent_version(&mut self, parent: OwnedRoomId) {
        *Arc::make_mut(&mut self.parent_versions)
            .entry(parent)
            .or_insert(0) += 1;
    }

    pub fn update_child_parents(
        &mut self,
        child_id: OwnedRoomId,
        new_parents: HashMap<OwnedRoomId, ChildOrder>,
    ) {
        let new_parents_set = new_parents
            .into_iter()
            .map(|(parent_id, order)| {
                Arc::make_mut(&mut self.parent_to_child_orders)
                    .entry(parent_id.clone())
                    .or_default()
                    .insert(child_id.clone(), order);

                parent_id
            })
            .collect();

        let old_parents_set = self
            .child_to_parents
            .get(&child_id)
            .cloned()
            .unwrap_or_default();

        if new_parents_set == old_parents_set {
            return;
        }

        let mut hierarchy_changed = false;

        for removed_parent in old_parents_set.difference(&new_parents_set) {
            if let Some(children) =
                Arc::make_mut(&mut self.parent_to_children).get_mut(removed_parent)
            {
                Arc::make_mut(children).retain(|id| id != &child_id);

                if children.is_empty() {
                    Arc::make_mut(&mut self.parent_to_children).remove(removed_parent);
                }
            }
            self.bump_parent_version(removed_parent.clone());
            hierarchy_changed = true;
        }

        for added_parent in new_parents_set.difference(&old_parents_set) {
            if let Some(children) =
                Arc::make_mut(&mut self.parent_to_children).get_mut(added_parent)
            {
                Arc::make_mut(children).push(child_id.clone());
            } else {
                Arc::make_mut(&mut self.parent_to_children)
                    .insert(added_parent.clone(), Arc::new(vec![child_id.clone()]));
            }

            self.bump_parent_version(added_parent.clone());
            hierarchy_changed = true;
        }

        if new_parents_set.is_empty() {
            Arc::make_mut(&mut self.child_to_parents).remove(&child_id);
        } else {
            for parent_id in &new_parents_set {
                self.sort_children(parent_id);
            }

            Arc::make_mut(&mut self.child_to_parents).insert(child_id, new_parents_set);
        }

        if hierarchy_changed {
            self.recompute_all_children();
            self.apply_server_order();
            self.total_version += 1;
        }
    }

    pub fn get_children(&self, parent: &RoomId) -> Arc<Vec<OwnedRoomId>> {
        self.parent_to_children
            .get(parent)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_all_children(&self, parent: &RoomId) -> Arc<BTreeSet<OwnedRoomId>> {
        self.parent_to_all_children
            .get(parent)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_server_of(&self, room_id: &RoomId) -> Option<OwnedRoomId> {
        for server in self.servers.iter() {
            if let Some(children) = self.parent_to_all_children.get(server)
                && children.contains(room_id)
            {
                return Some(server.clone());
            }
        }
        None
    }

    pub fn reorder_servers(&mut self, index: usize, target_index: usize) -> ServerOrderContent {
        let servers = Arc::make_mut(&mut self.servers);
        let server = servers.remove(index);
        servers.insert(target_index, server);
        let order = ServerOrderContent {
            servers: (*self.servers).clone(),
        };
        self.last_server_order = Arc::new(order.clone());
        order
    }

    pub fn get_first_child_of(&self, parent: &RoomId) -> Option<OwnedRoomId> {
        self.parent_to_children
            .get(parent)
            .and_then(|children| children.first().cloned())
    }
}
