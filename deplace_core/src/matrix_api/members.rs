use matrix_sdk::{Client, Room, RoomMemberships, event_handler::Ctx};
use ruma::events::room::member::OriginalSyncRoomMemberEvent;

use crate::state::{AppState, MembershipMap};

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

async fn set_membership_map(rooms: Vec<Room>, state: AppState) {
    let mut membership_map = MembershipMap::default();

    for room in rooms {
        let members = room
            .members(RoomMemberships::JOIN)
            .await
            .map_err(|e| {
                tracing::error!("Failed to get room members: {:?}", e);
            })
            .unwrap_or_default();
        membership_map.insert(room.room_id().into(), members);
    }
    state.set_membership_map(membership_map);
}

pub async fn run_membership_map_update(client: Client, state: AppState) {
    set_membership_map(client.rooms(), state).await;

    client.add_event_handler(handle_member_event);
}
