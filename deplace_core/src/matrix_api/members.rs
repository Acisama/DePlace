use indexmap::IndexMap;
use matrix_sdk::{Client, Room, RoomMemberships, event_handler::Ctx, room::RoomMember};
use ruma::{OwnedUserId, events::room::member::OriginalSyncRoomMemberEvent};

use crate::{
    ProfileLike,
    state::{AppState, MembershipMap},
};

async fn handle_member_event(ev: OriginalSyncRoomMemberEvent, room: Room, state: Ctx<AppState>) {
    let member = match room.get_member(&ev.sender).await {
        Ok(Some(member)) => member,
        Ok(None) => {
            tracing::warn!("Member not found: {:?}", ev.sender);
            return;
        }
        Err(e) => {
            tracing::error!("Failed to get room member: {:?}", e);
            return;
        }
    };

    state.add_membership(room.room_id().into(), member);
}

pub async fn set_membership_map(rooms: Vec<Room>, state: AppState) {
    let mut membership_map = MembershipMap::default();

    for room in rooms {
        let mut members: IndexMap<OwnedUserId, RoomMember> = room
            .members(RoomMemberships::ACTIVE)
            .await
            .map_err(|e| {
                tracing::error!("Failed to get room members: {:?}", e);
            })
            .unwrap_or_default()
            .into_iter()
            .map(|m| (m.user_id().to_owned(), m))
            .collect();
        members.sort_by_cached_key(|_, m| m.get_name());

        membership_map.insert(room.room_id().into(), members);
    }
    state.set_membership_map(membership_map);
}

pub async fn run_membership_map_update(client: Client, state: AppState) {
    set_membership_map(client.rooms(), state).await;

    client.add_event_handler(handle_member_event);
}
