use std::collections::HashSet;

use matrix_sdk::{Client, RoomMemberships};
use ruma::{OwnedUserId, events::presence::PresenceEvent, serde::Raw};

use crate::state::{AppState, PresenceMap};

pub fn handle_presences(events: &[Raw<PresenceEvent>], state: &AppState) {
    let presences: PresenceMap = events
        .iter()
        .filter_map(|raw_ev| {
            let ev = match raw_ev.deserialize() {
                Ok(ev) => ev,
                Err(_) => return None,
            };
            Some((ev.sender.clone(), ev.content.clone()))
        })
        .collect();

    state.add_presences(presences);
}

pub async fn get_presences(client: Client, state: AppState) {
    let mut user_ids = HashSet::new();

    for room in client.rooms() {
        let members = match room.members(RoomMemberships::all()).await {
            Ok(members) => members,
            Err(e) => {
                tracing::warn!("Failed to get members for room: {}", e);
                continue;
            }
        };

        user_ids.extend(members.iter().map(|m| m.user_id().to_owned()));
    }

    let user_ids: Vec<OwnedUserId> = user_ids.into_iter().collect();
    let presences = match client.state_store().get_presence_events(&user_ids).await {
        Ok(presences) => presences,
        Err(e) => {
            tracing::warn!("Failed to get presence events: {}", e);
            return;
        }
    };

    handle_presences(&presences, &state);
}
