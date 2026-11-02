use std::sync::Arc;

use dashmap::DashMap;
use indexmap::{IndexMap, map::Entry};
use matrix_sdk::Client;
use ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use tokio::sync::watch;

use super::{
    Member, Shared,
    profile::{ProfileData, load_avatar},
    user_data::UserData,
};

pub type RoomMemberMap = DashMap<OwnedRoomId, IndexMap<OwnedUserId, Arc<Shared<ProfileData>>>>;
pub type UserMap = DashMap<OwnedUserId, Arc<Shared<UserData>>>;

pub struct ProfileStore {
    client: Client,
    room_map: RoomMemberMap,
    user_map: UserMap,
    pub(crate) changed: watch::Sender<()>,
}

impl ProfileStore {
    pub fn new(client: &Client) -> Self {
        Self {
            client: client.clone(),
            room_map: RoomMemberMap::default(),
            user_map: UserMap::default(),
            changed: watch::channel(()).0,
        }
    }

    fn user_cell(&self, user_id: &UserId) -> Arc<Shared<UserData>> {
        if let Some(cell) = self.user_map.get(user_id) {
            return cell.clone();
        }

        self.user_map
            .entry(user_id.to_owned())
            .or_insert_with(|| Arc::new(Shared::new(UserData::new(user_id))))
            .clone()
    }

    fn room_cell(&self, room_id: &RoomId, user_id: &UserId) -> (Arc<Shared<ProfileData>>, bool) {
        {
            if let Some(room) = self.room_map.get(room_id)
                && let Some(cell) = room.get(user_id)
            {
                return (cell.clone(), false);
            }
        } // read guard must be dropped here, otherwise entry() deadlocks

        let mut room = self.room_map.entry(room_id.to_owned()).or_default();
        match room.entry(user_id.to_owned()) {
            Entry::Occupied(e) => (e.get().clone(), false), // someone else was faster
            Entry::Vacant(e) => {
                let cell = Arc::new(Shared::new(ProfileData::new(user_id)));
                e.insert(cell.clone());
                (cell, true)
            }
        }
    }

    fn spawn_member_load(
        &self,
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        cell: Arc<Shared<ProfileData>>,
    ) {
        let client = self.client.clone();
        let changed = self.changed.clone();

        tokio::spawn(async move {
            let Some(room) = client.get_room(&room_id) else {
                return;
            };
            let m = match room.get_member(&user_id).await {
                Ok(Some(m)) => m,
                Ok(None) => return,
                Err(e) => return tracing::warn!("get_member {user_id} in {room_id}: {e}"),
            };

            let data = ProfileData::from(&m);
            let url = data.avatar_url().cloned();
            cell.set(data);
            changed.send_modify(|_| ());

            if let Some(url) = url {
                load_avatar(&client, &cell, url).await;
                changed.send_modify(|_| ());
            }
        });
    }

    pub fn get_member(&self, user_id: &UserId, room_id: &RoomId) -> Member {
        let user = self.user_cell(user_id);
        let (room, created) = self.room_cell(room_id, user_id);

        if created {
            self.spawn_member_load(room_id.to_owned(), user_id.to_owned(), room.clone());
        }

        Member {
            user_id: user_id.to_owned(),
            room_id: room_id.to_owned(),
            room,
            user,
        }
    }
}
