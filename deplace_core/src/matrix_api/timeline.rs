use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use futures_util::StreamExt;
use matrix_sdk::Room;
use matrix_sdk_ui::timeline::{
    DateDividerMode, Timeline, TimelineBuilder, TimelineFocus, TimelineItem,
    TimelineReadReceiptTracking,
};
use ruma::{OwnedEventId, OwnedRoomId};
use tokio::{
    sync::watch::{self, Receiver},
    task::JoinHandle,
};
use uuid::Uuid;

type TimelineFocusMap = HashMap<(OwnedRoomId, Option<OwnedEventId>), (Arc<Timeline>, Uuid)>;
type TimelineMap = HashMap<Uuid, (Arc<Timeline>, bool)>;
pub type Messages = imbl::Vector<Arc<TimelineItem>>;

pub enum ScrollDirection {
    Up,
    Down,
}

impl std::fmt::Display for ScrollDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScrollDirection::Up => write!(f, "up"),
            ScrollDirection::Down => write!(f, "down"),
        }
    }
}

#[derive(Default, Clone)]
pub struct TimelineManager {
    timelines: Arc<Mutex<TimelineFocusMap>>,
    timelines_by_id: Arc<Mutex<TimelineMap>>,
    handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl TimelineManager {
    async fn get_or_create_timeline(
        &self,
        room: &Room,
        focus: TimelineFocus,
    ) -> Result<(Arc<Timeline>, Uuid)> {
        let event_id = if let TimelineFocus::Event { target, .. } = &focus {
            Some(target.clone())
        } else {
            None
        };
        let index = (room.room_id().to_owned(), event_id.clone());

        if let Some((timeline, id)) = self
            .timelines
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&index)
        {
            return Ok((timeline.clone(), *id));
        }

        let timeline = TimelineBuilder::new(room)
            .with_date_divider_mode(DateDividerMode::Daily)
            .with_focus(focus)
            .track_read_marker_and_receipts(TimelineReadReceiptTracking::AllEvents)
            .add_failed_to_parse(true)
            .build()
            .await?;

        if let Some(event_id) = event_id {
            timeline.fetch_details_for_event(&event_id).await?;
        } else {
            timeline.paginate_backwards(30).await?;
        }

        let timeline = Arc::new(timeline);
        let id = Uuid::new_v4();

        self.timelines
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(index, (timeline.clone(), id));
        self.timelines_by_id
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(id, (timeline.clone(), false));

        Ok((timeline, id))
    }

    async fn get_timeline_by_id(&self, id: Uuid) -> Option<(Arc<Timeline>, bool)> {
        self.timelines_by_id
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&id)
            .cloned()
    }

    pub async fn get_messages(
        &self,
        room: &Room,
        focus: TimelineFocus,
    ) -> Result<(Receiver<Messages>, Uuid)> {
        tracing::debug!(
            "Getting timeline with focues {:?} for room {}",
            focus.clone(),
            room.room_id()
        );
        let (timeline, id) = self.get_or_create_timeline(room, focus).await?;

        if let Some(handle) = &*self
            .handle
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
        {
            handle.abort();
        }

        let (initial_messages, mut update_stream) = timeline.subscribe().await;
        let (messages, receiver) = watch::channel(initial_messages);

        let handle = tokio::spawn(async move {
            loop {
                let Some(update) = update_stream.next().await else {
                    break;
                };
                messages.send_modify(|current| {
                    update.into_iter().for_each(|diff| diff.apply(current));
                });
            }
        });

        self.handle
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .replace(handle);

        Ok((receiver, id))
    }

    pub async fn scroll_timeline(&self, id: Uuid, direction: ScrollDirection) {
        tracing::debug!("Scrolling timeline {} {}", id, direction);
        let Some((timeline, reached_start)) = self.get_timeline_by_id(id).await else {
            tracing::error!("Timeline not found: {}", id);
            return;
        };

        if reached_start {
            return;
        }

        let res = match direction {
            ScrollDirection::Up => timeline.paginate_backwards(30).await,
            ScrollDirection::Down => timeline.paginate_forwards(30).await,
        };

        match res {
            Ok(reached_start) => {
                self.timelines_by_id
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .insert(id, (timeline.clone(), reached_start));
            }
            Err(e) => {
                tracing::error!("Failed to scroll timeline: {}", e);
                self.timelines_by_id
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .insert(id, (timeline, false));
            }
        }
    }
}
