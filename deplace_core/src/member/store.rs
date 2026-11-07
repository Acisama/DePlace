use std::{collections::HashSet, sync::Arc};

use dashmap::DashMap;
use matrix_sdk::{Client, Room, RoomMemberships, room::RoomMember};
use ruma::{OwnedMxcUri, OwnedRoomId, OwnedUserId, RoomId, UserId};
use tokio::sync::watch;

use crate::Shared;

use super::{
    Member,
    profile::ProfileData,
    room::RoomMembers,
    user_data::{CustomData, Presence, UserData},
};

#[cfg(feature = "iced")]
use crate::Avatar;

pub type RoomMemberMap = DashMap<OwnedRoomId, Arc<RoomMembers>>;
pub type UserMap = DashMap<OwnedUserId, (Arc<OwnedUserId>, Arc<Shared<UserData>>)>;

#[derive(Debug)]
pub struct ProfileStore {
    client: Client,
    handle: tokio::runtime::Handle,
    room_map: RoomMemberMap,
    user_map: UserMap,
    pub(crate) changed: watch::Sender<()>,
}

impl ProfileStore {
    pub fn new(client: &Client) -> Self {
        Self {
            client: client.clone(),
            handle: tokio::runtime::Handle::current(),
            room_map: RoomMemberMap::default(),
            user_map: UserMap::default(),

            changed: watch::channel(()).0,
        }
    }

    pub(crate) fn user_cell(&self, user_id: &UserId) -> (Arc<OwnedUserId>, Arc<Shared<UserData>>) {
        if let Some(stuff) = self.user_map.get(user_id) {
            return stuff.clone();
        }

        let mut created = false;
        let stuff = self
            .user_map
            .entry(user_id.to_owned())
            .or_insert_with(|| {
                created = true;
                (
                    Arc::new(user_id.to_owned()),
                    Arc::new(Shared::new(UserData::new(user_id))),
                )
            })
            .clone();

        if created {
            self.spawn_custom_data_load(user_id.to_owned(), stuff.1.clone());
        }

        stuff
    }

    pub fn get_presence(&self, user_id: &UserId) -> Presence {
        let (_, user_data) = self.user_cell(user_id);
        user_data.read().presence
    }

    fn spawn_custom_data_load(&self, user_id: OwnedUserId, user_data: Arc<Shared<UserData>>) {
        let client = self.client.clone();
        let changed = self.changed.clone();

        self.handle.spawn(async move {
            let custom_data = CustomData::load(&client, &user_id).await;
            user_data.change(|ud| ud.custom_data = custom_data.clone());
            changed.send_modify(|_| ());
        });
    }

    pub(crate) fn update_room_profile(
        &self,
        room_id: &RoomId,
        user_id: &UserId,
        f: impl Fn(&mut ProfileData),
    ) {
        let room_members = self.get_members(room_id);
        let member = self.get_member(room_id, user_id);
        room_members.update_member(&member, f);
    }

    fn spawn_member_load(
        &self,
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        cell: Arc<Shared<ProfileData>>,
    ) {
        let client = self.client.clone();
        let changed = self.changed.clone();

        self.handle.spawn(async move {
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

            if cfg!(feature = "iced") {
                let url_clone = url.clone();
                let changed_clone = changed.clone();
                let client_clone = client.clone();
                let cell_clone = cell.clone();
                futures::future::join(
                    async move {
                        let avatar = Avatar::load_avatar(&client, url.clone(), false).await;
                        cell.change(|d| d.avatar_handle = avatar.clone());
                        changed.send_modify(|_| ());
                    },
                    async move {
                        let small_avatar =
                            Avatar::load_avatar(&client_clone, url_clone, true).await;
                        cell_clone.change(|d| d.small_avatar_handle = small_avatar.clone());
                        changed_clone.send_modify(|_| ());
                    },
                )
                .await;
            }
        });
    }

    #[cfg(feature = "iced")]
    fn spawn_avatar_load(&self, cell: Arc<Shared<ProfileData>>, url: Option<OwnedMxcUri>) {
        let client = self.client.clone();
        let changed = self.changed.clone();
        let url_clone = url.clone();
        let changed_clone = changed.clone();
        let client_clone = client.clone();
        let cell_clone = cell.clone();
        self.handle.spawn(async move {
            let avatar = Avatar::load_avatar(&client, url.clone(), false).await;
            cell.change(|d| d.avatar_handle = avatar.clone());
            changed.send_modify(|_| ());
        });
        self.handle.spawn(async move {
            let small_avatar = Avatar::load_avatar(&client_clone, url_clone, true).await;
            cell_clone.change(|d| d.small_avatar_handle = small_avatar.clone());
            changed_clone.send_modify(|_| ());
        });
    }

    pub fn get_member(&self, room_id: &RoomId, user_id: &UserId) -> Member {
        let room_members = self.get_members(room_id);

        if let Some(member) = room_members.get(user_id) {
            return member.clone();
        }

        let (user_id_arc, user_data) = self.user_cell(user_id);
        let (member, created) = room_members.get_or_create(user_id_arc, user_data);

        if created {
            self.spawn_member_load(
                room_id.to_owned(),
                user_id.to_owned(),
                member.room_data.clone(),
            );
        }

        member
    }

    pub fn get_members(&self, room_id: &RoomId) -> Arc<RoomMembers> {
        self.room_map
            .entry(room_id.to_owned())
            .or_insert_with(|| Arc::new(RoomMembers::new(room_id.to_owned())))
            .clone()
    }

    /// Re-sorts/re-partitions every room's cached member list from already-loaded
    /// data. Meant to be called once after a bulk background member load finishes,
    /// to settle any room that missed a rebuild due to loads interleaving across
    /// rooms (e.g. a presence update landing between two other rooms' loads).
    pub fn recalculate_all_rooms(&self) {
        for room_members in self.room_map.iter() {
            room_members.value().recalculate();
        }
    }

    pub async fn load_room_members(&self, room: &Room) -> HashSet<OwnedUserId> {
        let members = room
            .members(RoomMemberships::ACTIVE)
            .await
            .unwrap_or_default();

        self.insert_new_room_members(room.room_id(), members)
    }

    pub(crate) fn insert_new_room_members(
        &self,
        room_id: &RoomId,
        members: Vec<RoomMember>,
    ) -> HashSet<OwnedUserId> {
        let room_members = self.get_members(room_id);

        let mut new_members = Vec::new();
        let mut user_ids = HashSet::new();

        for member in &members {
            let user_id = member.user_id();
            user_ids.insert(user_id.to_owned());

            // Already known in this room
            if room_members.get(user_id).is_some() {
                continue;
            }

            let (_, user_data) = self.user_cell(user_id);
            let profile_data = ProfileData::from(member);
            let url = profile_data.avatar_url().cloned();
            let room_data = Arc::new(Shared::new(profile_data));

            #[cfg(feature = "iced")]
            self.spawn_avatar_load(room_data.clone(), url);

            let member = Member {
                user_id: Arc::new(user_id.to_owned()),
                room_id: Arc::new(room_id.to_owned()),
                room_data,
                user_data,
            };

            new_members.push(member);
        }

        room_members.extend(new_members);

        user_ids
    }
}
