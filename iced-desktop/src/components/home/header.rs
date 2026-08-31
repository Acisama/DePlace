use crate::common::*;

#[derive(Clone)]
pub struct Header {
    state: AppState,
    avatar_cache: AvatarCache,

    membership_map: MembershipMap,

    active_room: Receiver<Option<Room>>,
}
