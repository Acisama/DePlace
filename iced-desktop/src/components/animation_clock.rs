use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Seconds since the app's first frame.
///
/// Every animated background shader (the full-screen one and the tile blur
/// source) reads its `time` uniform from here, rather than each keeping its
/// own clock started at whenever its pipeline happened to be constructed.
/// Otherwise the two renders of the same pattern drift out of phase and their
/// lines visibly don't line up.
pub(crate) fn elapsed_seconds() -> f32 {
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}
