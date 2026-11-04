use std::sync::Arc;

use matrix_sdk::Room;
use matrix_sdk_ui::Timeline;
use matrix_sdk_ui::timeline::{
    DateDividerMode, TimelineBuilder, TimelineFocus, TimelineReadReceiptTracking,
};

/// The live and pinned-events timelines for a room, built once and cached on
/// `DePlaceRoom` -- a `Timeline` stays hooked into the room's live sync
/// pipeline for as long as it exists, so building it is worth doing only
/// once per room rather than every time a chat view is opened.
#[derive(Debug, Clone)]
pub struct RoomTimelines {
    pub live: Arc<Timeline>,
    pub pinned: Arc<Timeline>,
}

impl RoomTimelines {
    pub(crate) async fn build(room: &Room) -> anyhow::Result<Self> {
        let live_builder = TimelineBuilder::new(room)
            .with_date_divider_mode(DateDividerMode::Daily)
            .with_focus(TimelineFocus::Live {
                hide_threaded_events: false,
            })
            .track_read_marker_and_receipts(TimelineReadReceiptTracking::MessageLikeEvents)
            .add_failed_to_parse(true);

        let pinned_builder = TimelineBuilder::new(room)
            .with_date_divider_mode(DateDividerMode::Daily)
            .with_focus(TimelineFocus::PinnedEvents)
            .track_read_marker_and_receipts(TimelineReadReceiptTracking::Disabled)
            .add_failed_to_parse(true);

        let (live, pinned) = tokio::try_join!(live_builder.build(), pinned_builder.build())?;

        Ok(Self {
            live: Arc::new(live),
            pinned: Arc::new(pinned),
        })
    }
}
