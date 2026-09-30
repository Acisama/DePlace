use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
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

static STATE: AtomicU32 = AtomicU32::new(0);
static PREV_STATE: AtomicU32 = AtomicU32::new(0);
static LAST_CHANGED_TIME: AtomicU32 = AtomicU32::new(0);

/// Publishes the loading shader's current `state`/`prev_state`/`last_changed_time`
/// so the tile blur source can render the same screen-transition state as the
/// visible full-screen background, rather than a fixed state of its own.
pub(crate) fn set_screen_state(state: f32, prev_state: f32, last_changed_time: f32) {
    STATE.store(state.to_bits(), Ordering::Relaxed);
    PREV_STATE.store(prev_state.to_bits(), Ordering::Relaxed);
    LAST_CHANGED_TIME.store(last_changed_time.to_bits(), Ordering::Relaxed);
}

/// Returns the `(state, prev_state, last_changed_time)` most recently published
/// by [`set_screen_state`].
pub(crate) fn screen_state() -> (f32, f32, f32) {
    (
        f32::from_bits(STATE.load(Ordering::Relaxed)),
        f32::from_bits(PREV_STATE.load(Ordering::Relaxed)),
        f32::from_bits(LAST_CHANGED_TIME.load(Ordering::Relaxed)),
    )
}
