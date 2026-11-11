pub trait IsActiveRoom {}
impl IsActiveRoom for tokio::sync::watch::Receiver<Option<crate::DePlaceRoom>> {}

pub trait IsActiveServer {}
impl IsActiveServer for tokio::sync::watch::Receiver<crate::state::ActiveServer> {}

/// Implemented by `#[iced_cache]` structs that need to fold extra state into their
/// `Hash` impl beyond their `#[hash]`-marked fields. Picked up automatically by the
/// macro if implemented; no-op otherwise.
pub trait ExtraHash {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H);
}
