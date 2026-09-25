use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use ruma::{OwnedRoomId, RoomId};

use crate::matrix_api::account_data::ServerOrderContent;

#[derive(Clone, Debug)]
pub struct SpaceHierarchy {
    pub parent_to_children: Arc<BTreeMap<OwnedRoomId, Arc<Vec<OwnedRoomId>>>>,
    pub parent_to_all_children: Arc<BTreeMap<OwnedRoomId, Arc<BTreeSet<OwnedRoomId>>>>,
    pub child_to_parents: Arc<BTreeMap<OwnedRoomId, BTreeSet<OwnedRoomId>>>,

    pub parent_to_child_orders: Arc<BTreeMap<OwnedRoomId, BTreeMap<OwnedRoomId, String>>>,

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
        let mut all_parents: HashSet<OwnedRoomId> =
            self.parent_to_children.keys().cloned().collect();

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
                        all_parents.remove(child);
                        stack.push(child.clone());
                    }
                }
            }

            new_all_children.insert(parent_id.clone(), Arc::new(all_kids.into_iter().collect()));
        }

        self.servers = Arc::new(all_parents.into_iter().collect());
        self.parent_to_all_children = Arc::new(new_all_children);
    }

    fn sort_children(&mut self, parent: &RoomId) {
        if let Some(children) = Arc::make_mut(&mut self.parent_to_children).get_mut(parent) {
            Arc::make_mut(children).sort_by_key(|id| {
                self.parent_to_child_orders
                    .get(id)
                    .and_then(|map| map.get(id))
            });
        }
    }

    fn apply_server_order(&mut self) {
        if self.last_server_order.servers == *self.servers {
            return;
        }

        let mut all_servers: BTreeSet<OwnedRoomId> = self.servers.iter().cloned().collect();
        let mut new_server_order = Vec::new();

        for server in &self.last_server_order.servers {
            new_server_order.push(server.clone());
            if self.servers.contains(server) {
                all_servers.remove(server);
            }
        }

        new_server_order.extend(all_servers);
        self.servers = Arc::new(new_server_order.clone());
        self.last_server_order = Arc::new(ServerOrderContent {
            servers: new_server_order,
        });
    }

    fn bump_parent_version(&mut self, parent: OwnedRoomId) {
        *Arc::make_mut(&mut self.parent_versions)
            .entry(parent)
            .or_insert(0) += 1;
    }

    pub fn update_child_parents(
        &mut self,
        child_id: OwnedRoomId,
        new_parents: HashMap<OwnedRoomId, Option<String>>,
    ) {
        let new_parents_set = new_parents
            .into_iter()
            .map(|(parent_id, order)| {
                if let Some(order) = order {
                    Arc::make_mut(&mut self.parent_to_child_orders)
                        .get_mut(&parent_id)
                        .and_then(|map| map.insert(child_id.clone(), order));
                }

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
        Arc::make_mut(&mut self.servers).swap(index, target_index);
        ServerOrderContent {
            servers: (*self.servers).clone(),
        }
    }

    pub fn get_first_child_of(&self, parent: &RoomId) -> Option<OwnedRoomId> {
        self.parent_to_children
            .get(parent)
            .and_then(|children| children.first().cloned())
    }
}
