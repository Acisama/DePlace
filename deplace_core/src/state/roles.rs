pub trait IsActiveRoom {}
impl IsActiveRoom for tokio::sync::watch::Receiver<Option<crate::DePlaceRoom>> {}

pub trait IsActiveServer {}
impl IsActiveServer for tokio::sync::watch::Receiver<crate::state::ActiveServer> {}

pub trait IsAvatarCache {}
impl IsAvatarCache for crate::state::cache::AvatarCache {}

pub trait IsThumbnailCache {}
impl IsThumbnailCache for crate::state::cache::ThumbnailCache {}

pub trait IsVideoCache {}
impl IsVideoCache for crate::state::cache::VideoCache {}

pub trait IsImageCache {}
impl IsImageCache for crate::state::cache::ImageCache {}

pub trait IsMembershipMap {}
impl IsMembershipMap for tokio::sync::watch::Receiver<crate::state::MembershipMap> {}

pub trait IsPresenceMap {}
impl IsPresenceMap for tokio::sync::watch::Receiver<crate::state::PresenceMap> {}

pub trait IsRoomWatchers {}
impl IsRoomWatchers for crate::state::RoomWatchers {}

/// Implemented by `#[iced_cache]` structs that need to fold extra state into their
/// `Hash` impl beyond their `#[hash]`-marked fields. Picked up automatically by the
/// macro if implemented; no-op otherwise.
pub trait ExtraHash {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H);
}
