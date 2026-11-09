use matrix_sdk::{Room, event_handler::Ctx};
use ruma::events::room::member::OriginalSyncRoomMemberEvent;

use crate::{member::profile::ProfileData, state::AppState};

pub(crate) async fn handle_member_event(
    ev: OriginalSyncRoomMemberEvent,
    room: Room,
    state: Ctx<AppState>,
) {
    let room_id = room.room_id();

    let member = state.profile_store_ref().get_member(room_id, &ev.sender);

    let new_displayname = &ev.content.displayname;
    let new_avatar_url = &ev.content.avatar_url;
    let new_membership = &ev.content.membership;
    let new_power_level = room
        .get_user_power_level(&ev.sender)
        .await
        .map_err(|e| tracing::error!("Failed to get powerlevels in room {}: {e}", room_id))
        .ok();

    let change_displayname = &member.room_data.data.load().displayname != new_displayname;
    let change_avatar_url = &member.room_data.data.load().avatar_url != new_avatar_url;
    let change_membership = &member.room_data.data.load().membership != new_membership;
    let change_power_level =
        new_power_level.is_some_and(|level| member.room_data.data.load().power_level != level);

    if !change_displayname && !change_avatar_url && !change_membership && !change_power_level {
        return;
    }

    let update_fn = |data: &mut ProfileData| {
        if change_displayname {
            data.displayname = new_displayname.clone();
            data.calculate_strings();
        }
        if change_avatar_url {
            data.reload_avatars(new_avatar_url.clone(), state.better_thumbnail_cache());
        }
        if change_membership {
            data.membership = new_membership.clone();
        }
        if change_power_level && let Some(level) = new_power_level {
            data.power_level = level;
        }
    };

    state
        .profile_store_ref()
        .update_room_profile(room_id, &ev.sender, update_fn);
}
