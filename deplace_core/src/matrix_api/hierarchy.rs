use matrix_sdk::{Client, Room, event_handler::Ctx};
use ruma::events::space::child::OriginalSyncSpaceChildEvent;

use crate::state::AppState;

/// `m.space.child` is a state event on the *parent* space room, keyed by the
/// child room's ID - `ev.state_key` is the child, `room` is the parent. The
/// hierarchy itself is a property of the child room's `parent_spaces()`, so a
/// change here means that child's position needs refreshing.
async fn handle_space_child_event(
    ev: OriginalSyncSpaceChildEvent,
    _room: Room,
    state: Ctx<AppState>,
) {
    state
        .room_store_ref()
        .refresh_hierarchy(&ev.state_key)
        .await;
}

pub fn run_space_hierarchy_update(client: &Client) {
    client.add_event_handler(handle_space_child_event);
}
