pub trait IsActiveRoom {}
impl IsActiveRoom for tokio::sync::watch::Receiver<Option<crate::Room>> {}

pub trait IsActiveServer {}
impl IsActiveServer for tokio::sync::watch::Receiver<crate::state::ActiveServer> {}

pub trait IsAvatarCache {}
impl IsAvatarCache for crate::state::cache::AvatarCache {}

pub trait IsMembershipMap {}
impl IsMembershipMap for tokio::sync::watch::Receiver<crate::state::MembershipMap> {}

pub trait IsPresenceMap {}
impl IsPresenceMap for tokio::sync::watch::Receiver<crate::state::PresenceMap> {}

pub trait IsRoomDependency {}
impl IsRoomDependency for tokio::sync::watch::Receiver<crate::state::RoomMap> {}
impl IsRoomDependency for tokio::sync::watch::Receiver<crate::state::ParentToChildren> {}
impl IsRoomDependency for tokio::sync::watch::Receiver<crate::state::ParentToChildrenOrderStr> {}
