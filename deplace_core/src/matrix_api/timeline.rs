use std::{fmt::Display, sync::Arc};

use anyhow::Result;
use dashmap::DashMap;
use futures_util::Stream;
use matrix_sdk::Room;
use matrix_sdk_ui::{
    eyeball_im::VectorDiff,
    timeline::{
        DateDividerMode, MemberProfileChange, Timeline, TimelineBuilder, TimelineEventItemId,
        TimelineFocus, TimelineItem, TimelineReadReceiptTracking,
    },
};
use ruma::{
    OwnedEventId, OwnedRoomId,
    events::{
        RedactContent, StateEventContentChange, StaticStateEventContent, room::member::Change,
    },
};
use uuid::Uuid;

type TimelineFocusMap = DashMap<(OwnedRoomId, Option<OwnedEventId>), (Arc<Timeline>, Uuid)>;
type TimelineMap = DashMap<Uuid, (Arc<Timeline>, PaginationState)>;
pub type Messages = Vec<Arc<TimelineItem>>;

#[derive(Default, Clone, Copy)]
struct PaginationState {
    reached_start: bool,
    reached_end: bool,
}

#[derive(Clone, Copy)]
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
    timelines: Arc<TimelineFocusMap>,
    timelines_by_id: Arc<TimelineMap>,
}

impl TimelineManager {
    pub async fn get_or_create_timeline(
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

        if let Some((timeline, id)) = self.timelines.get(&index).map(|t| t.clone()) {
            return Ok((timeline, id));
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

        self.timelines.insert(index, (timeline.clone(), id));
        self.timelines_by_id
            .insert(id, (timeline.clone(), PaginationState::default()));

        Ok((timeline, id))
    }

    async fn get_timeline_by_id(&self, id: Uuid) -> Option<(Arc<Timeline>, PaginationState)> {
        self.timelines_by_id.get(&id).map(|t| t.clone())
    }

    pub async fn get_messages(
        &self,
        room: &Room,
        focus: TimelineFocus,
    ) -> Result<(
        Messages,
        impl Stream<Item = Vec<VectorDiff<Arc<TimelineItem>>>> + use<>,
        Uuid,
    )> {
        tracing::debug!(
            "Getting timeline with focues {:?} for room {}",
            focus.clone(),
            room.room_id()
        );
        let (timeline, id) = self.get_or_create_timeline(room, focus).await?;
        let (initial_messages, update_stream) = timeline.subscribe().await;

        Ok((initial_messages.into_iter().collect(), update_stream, id))
    }

    pub async fn scroll_timeline(&self, id: Uuid, direction: ScrollDirection) {
        let Some((timeline, state)) = self.get_timeline_by_id(id).await else {
            tracing::error!("Timeline not found: {}", id);
            return;
        };

        let already_reached = match direction {
            ScrollDirection::Up => state.reached_start,
            ScrollDirection::Down => state.reached_end,
        };
        if already_reached {
            return;
        }

        tracing::debug!("Scrolling timeline {} {}", id, direction);
        let res = match direction {
            ScrollDirection::Up => timeline.paginate_backwards(30).await,
            ScrollDirection::Down => timeline.paginate_forwards(30).await,
        };

        let mut state = state;
        match res {
            Ok(reached) => {
                tracing::debug!(
                    "Scrolled timeline {} {}: reached={}",
                    id,
                    direction,
                    reached
                );
                match direction {
                    ScrollDirection::Up => state.reached_start = reached,
                    ScrollDirection::Down => state.reached_end = reached,
                }
            }
            Err(e) => {
                tracing::error!("Failed to scroll timeline: {}", e);
            }
        }

        self.timelines_by_id.insert(id, (timeline, state));
    }

    pub async fn toggle_reaction(&self, timeline_id: Uuid, event_id: OwnedEventId, reaction: &str) {
        let Some((timeline, _)) = self.timelines_by_id.get(&timeline_id).map(|t| t.clone()) else {
            tracing::warn!("Timeline for reaction not found: {}", timeline_id);
            return;
        };
        if let Err(e) = timeline
            .toggle_reaction(&TimelineEventItemId::EventId(event_id), reaction)
            .await
        {
            tracing::error!("Failed to toggle reaction: {}", e);
        }
    }
}

pub trait DisplayString {
    fn display_string(&self) -> String;
}

impl DisplayString for MemberProfileChange {
    fn display_string(&self) -> String {
        let mut changes = Vec::new();

        if let Some(Change { old, new }) = self.displayname_change() {
            if let Some(new) = new {
                if let Some(old) = old {
                    changes.push(format!(
                        "changed their display name from '{}' to '{}'",
                        old, new
                    ));
                } else {
                    changes.push(format!("set their display name to '{}'", new));
                }
            } else {
                changes.push("removed their display name".to_string());
            }
        }

        if let Some(Change { old, new }) = &self.avatar_url_change() {
            if new.is_some() && old.is_none() {
                changes.push("set a profile picture".to_string());
            } else if new.is_none() && old.is_some() {
                changes.push("removed their profile picture".to_string());
            } else {
                changes.push("changed their profile picture".to_string());
            }
        }

        changes.join(" and ")
    }
}

pub enum EventChange<T> {
    Unset(T),
    Set(T),
    Changed { old: T, new: T },
    Something,
}

impl<T> EventChange<T> {
    pub fn display_string_with_render_fn<C: Display>(
        &self,
        render_fn: impl Fn(&T) -> C,
        property_name: &str,
    ) -> String {
        match self {
            EventChange::Unset(_) => format!("unset {}", property_name),
            EventChange::Set(_) => format!("set {}", property_name),
            EventChange::Changed { old, new } => {
                format!(
                    "changed {} from {} to {}",
                    property_name,
                    render_fn(old),
                    render_fn(new)
                )
            }
            EventChange::Something => format!("changed {}", property_name),
        }
    }
}

impl<T: Display> EventChange<T> {
    pub fn display_string(&self, property_name: &str) -> String {
        match self {
            EventChange::Unset(_) => format!("unset {}", property_name),
            EventChange::Set(_) => format!("set {}", property_name),
            EventChange::Changed { old, new } => {
                format!("changed {} from {} to {}", property_name, old, new)
            }
            EventChange::Something => format!("changed {}", property_name),
        }
    }
}

pub fn get_current_and_prev<T: RedactContent + Clone + StaticStateEventContent, C>(
    change: &StateEventContentChange<T>,
    get_val1: impl FnOnce(&T) -> Option<C>,
    get_val2: impl FnOnce(&<T as StaticStateEventContent>::PossiblyRedacted) -> Option<C>,
) -> EventChange<C> {
    if let StateEventContentChange::Original {
        content,
        prev_content,
    } = change
    {
        let current = get_val1(content);
        let prev = prev_content.as_ref().and_then(get_val2);

        match (current, prev) {
            (Some(current), Some(prev)) => EventChange::Changed {
                old: prev,
                new: current,
            },
            (Some(current), None) => EventChange::Set(current),
            (None, Some(prev)) => EventChange::Unset(prev),
            (None, None) => EventChange::Something,
        }
    } else {
        EventChange::Something
    }
}

#[macro_export]
macro_rules! get_change {
    ($change:expr, |$arg:ident| $body:expr) => {
        $crate::matrix_api::timeline::get_current_and_prev(
            $change,
            |$arg| $body, // The compiler type-checks $arg as &T here
            |$arg| $body, // The compiler type-checks $arg as &T::PossiblyRedacted here
        )
    };
}
