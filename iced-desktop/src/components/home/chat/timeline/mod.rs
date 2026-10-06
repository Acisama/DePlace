use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use chrono_tz::Tz;
use deplace_core::PaginationDirection;
use deplace_core::settings::{DataSizeUnit, DateFormat, HourFormat, SystemMessageType};
use enumset::EnumSet;
use iced::Vector;
use iced::advanced::widget::operate;
use iced::advanced::widget::operation::scrollable::{RelativeOffset, snap_to};
use iced::widget::operation::scroll_to;
use macros::iced_cache;
use matrix_sdk::{
    media::UniqueKey,
    ruma::{
        events::{MessageLikeEventType, StateEventType, room::power_levels::RoomPowerLevels},
        room_version_rules::AuthorizationRules,
    },
};
use matrix_sdk_ui::{Timeline, eyeball_im::VectorDiff, timeline::TimelineItem as UiTimelineItem};
use messages::{
    MessageEvent, TimelineItem, TimelineItemAction, TimelineItemMessage, ToTimelineItem,
};
use sweeten::scrollable::AbsoluteOffset;
use sweeten::widget::list;

use phosphor_svgs::icon as icons;

use crate::components::home::overlay::{ContextMenu, ContextMenuKind, ModifyItem};
use crate::components::phosphor_icon;
use crate::{
    common::*,
    components::{track_bounds::track_bounds, track_scroll::track_scroll},
};

pub mod messages;

const LIST_ID: iced::widget::Id = iced::widget::Id::new("list");

#[derive(Debug, Clone)]
pub enum TimelineMessage {
    Item {
        id: String,
        message: TimelineItemMessage,
    },
    PinnedEventIds(BTreeSet<OwnedEventId>),
    Loaded {
        timeline: Arc<Timeline>,
        initial: Arc<IndexMap<String, Arc<TimelineItem>>>,
        power_levels: Arc<RoomPowerLevels>,
    },
    Diffs(Vec<VectorDiff<Arc<UiTimelineItem>>>),
    None,
    Scroll {
        direction: Option<PaginationDirection>,
        position: RelativeOffset,
    },
    ScrolledFromTop(f32),
    ButtonsHovered {
        id: String,
        hovered: bool,
    },
    ChatAreaBounds(Rectangle),
    ContextMenu(ContextMenu),
    /// Sent whenever this room becomes the active one (whether freshly
    /// created or reused from the LRU cache) to explicitly snap the
    /// scrollable back to where this room was left -- its own widget state
    /// is tied to tree position, not to which room is showing, so without
    /// this it just keeps whatever position the previously active room left
    /// behind.
    RestoreScrollPosition,
    KeyboardEvent(iced::keyboard::Event),
    ScrollTo(Option<Rectangle>),
    HelpHover(Option<HelpKey>),
}

pub enum TimelineAction {
    NeedsMedia(NeedsMedia),
    SetIsReplyingTo {
        message: Arc<MessageEvent>,
        event_id: OwnedEventId,
    },
    Run(Task<()>),
    Perform(Task<TimelineMessage>),
    Scroll {
        direction: PaginationDirection,
        task: Task<bool>,
    },
    OpenProfileOverlay {
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        bounds: Rectangle,
    },
    HelpHover(Option<HelpKey>),
    ContextMenu(ContextMenu),
    OpenModifyItem(ModifyItem),
}
const SCROLLABLE_ID: iced::widget::Id = iced::widget::Id::new("timeline-scrollable");

#[iced_cache(Clone)]
pub struct ChatTimeline {
    state: AppState,
    timeline: Option<Arc<Timeline>>,

    membership_map: Receiver<MembershipMap>,

    // TODO: Add logic to update this
    power_levels: Arc<RoomPowerLevels>,
    user_can_send: bool,
    user_can_pin: bool,
    user_can_redact_own: bool,
    user_can_redact_other: bool,

    // Stuff the messages depend on
    system_messages_to_show: Receiver<EnumSet<SystemMessageType>>,
    timezone: Receiver<Tz>,
    hour_format: Receiver<HourFormat>,
    date_format: Receiver<DateFormat>,
    data_size_unit: Receiver<DataSizeUnit>,

    scrolled_from_top: f32,
    /// Where this room's scrollable was left, so it can be restored
    scroll_position: RelativeOffset,
    restored_scroll: bool,

    #[hash]
    room_id: OwnedRoomId,
    own_user_id: OwnedUserId,

    #[hash]
    previous_pinned_event_ids: BTreeSet<OwnedEventId>,

    avatar_cache: AvatarCache,
    thumbnail_cache: ThumbnailCache,
    video_cache: VideoCache,

    reached_top: bool,
    reached_bottom: bool,
    loading_top: bool,
    loading_bottom: bool,

    #[hash]
    hovered_item_id: Option<String>,
    #[hash]
    buttons_hovered: bool,
    #[hash]
    replying_to: Option<String>,

    message_event_bounds: HashMap<String, Rectangle>,
    tile_bounds: Rectangle,

    content: list::Content<String, Arc<TimelineItem>>,
    #[hash]
    messages_version: u64,

    #[hash]
    focused_message_id: Option<String>,
}

impl ExtraHash for ChatTimeline {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.scrolled_from_top.to_bits().hash(state);
        self.tile_bounds.x.to_bits().hash(state);
        self.tile_bounds.y.to_bits().hash(state);
        self.tile_bounds.width.to_bits().hash(state);
        self.tile_bounds.height.to_bits().hash(state);
        self.message_event_bounds.len().hash(state);

        self.system_messages_to_show.borrow().hash(state);
        self.timezone.borrow().hash(state);
        self.hour_format.borrow().hash(state);
        self.date_format.borrow().hash(state);
        self.data_size_unit.borrow().hash(state);

        // sort for deterministic hashing
        let mut sorted_keys: Vec<&String> = self.message_event_bounds.keys().collect();
        sorted_keys.sort_unstable();

        for key in sorted_keys {
            if let Some(bounds) = self.message_event_bounds.get(key) {
                key.hash(state);
                bounds.x.to_bits().hash(state);
                bounds.y.to_bits().hash(state);
                bounds.width.to_bits().hash(state);
                bounds.height.to_bits().hash(state);
            }
        }
    }
}

impl ChatTimeline {
    pub fn new(
        state: &AppState,
        room: &DePlaceRoom,
        initial_pinned_event_ids: BTreeSet<OwnedEventId>,
    ) -> Self {
        let power_levels = Arc::new(RoomPowerLevels::new(
            matrix_sdk::ruma::events::room::power_levels::RoomPowerLevelsSource::None,
            &AuthorizationRules::V12,
            [],
        ));
        let own_user_id = room.own_user_id().to_owned();

        let settings = state.settings();

        let mut timeline = Self {
            state: state.clone(),
            timeline: None,
            membership_map: state.membership_map(),

            system_messages_to_show: settings.system_messages_to_show.watch(),
            timezone: settings.timezone.watch(),
            hour_format: settings.hour_format.watch(),
            date_format: settings.date_format.watch(),
            data_size_unit: settings.data_size_unit.watch(),

            power_levels,
            user_can_send: false,
            user_can_pin: false,
            user_can_redact_own: false,
            user_can_redact_other: false,

            scrolled_from_top: 0.0,
            // `RelativeOffset` is anchor-relative: 0.0 (`START`) means "at
            // rest, at the anchor" -- the bottom, for `anchor_bottom()`.
            scroll_position: RelativeOffset::START,
            restored_scroll: false,

            room_id: room.room_id().to_owned(),
            own_user_id,

            previous_pinned_event_ids: initial_pinned_event_ids,

            avatar_cache: state.avatar_cache().clone(),
            thumbnail_cache: state.thumbnail_cache().clone(),
            video_cache: state.video_cache().clone(),

            avatar_states_for_hash: BTreeSet::new(),
            thumbnail_states_for_hash: BTreeSet::new(),
            video_states_for_hash: BTreeSet::new(),

            reached_top: false,
            reached_bottom: false,
            loading_top: false,
            loading_bottom: false,

            hovered_item_id: None,
            buttons_hovered: false,
            replying_to: None,

            message_event_bounds: HashMap::new(),
            // No-op until the first real measurement comes in via `ChatAreaBounds`.
            tile_bounds: Rectangle {
                x: -1_000_000.0,
                y: -1_000_000.0,
                width: 2_000_000.0,
                height: 2_000_000.0,
            },

            content: list::Content::default(),
            messages_version: 0,
            focused_message_id: None,
        };
        timeline.recalculate_with_power_levels();
        timeline
    }

    fn restore_scroll_task(&self) -> Task<()> {
        operate(snap_to(
            SCROLLABLE_ID,
            self.scroll_position.into(),
            w::operation::Animation::Smooth,
        ))
    }

    pub fn remove_replying(&mut self) {
        if let Some(item_id) = self.replying_to.as_ref()
            && let Some(item) = self.content.get_mut(item_id)
        {
            Arc::make_mut(item).remove_replying();
        }
        self.replying_to = None;
    }

    // TODO: Actually use this
    pub fn recalculate_with_power_levels(&mut self) {
        self.user_can_send = self
            .power_levels
            .user_can_send_message(&self.own_user_id, MessageLikeEventType::RoomMessage);
        self.user_can_pin = self
            .power_levels
            .user_can_send_state(&self.own_user_id, StateEventType::RoomPinnedEvents);
        self.user_can_redact_own = self
            .power_levels
            .user_can_redact_own_event(&self.own_user_id);
        self.user_can_redact_other = self
            .power_levels
            .user_can_redact_event_of_other(&self.own_user_id);
    }

    /// The virtualized `List` only re-examines a row's hash when `get_mut`
    /// queues a `Change::Updated` for it; this method pokes every row to trigger
    /// re-examination when the media is actually loaded.
    pub fn touch_media(&mut self, media: &MediaLoaded) {
        let relevant = match media {
            MediaLoaded::Avatar { uri } => self.avatar_states_for_hash.contains(uri),
            MediaLoaded::Thumbnail { key } => self.thumbnail_states_for_hash.contains(key),
            MediaLoaded::Video { key } => self.video_states_for_hash.contains(key),
        };

        if !relevant {
            return;
        }

        for index in 0..self.content.len() {
            self.content.get_index_mut(index);
        }
    }

    /// Pokes every row to trigger re-examination -- see `touch_media` for
    /// why this is necessary. Used when something outside `self.content`
    /// that every row's view depends on changes, e.g. `system_messages_to_show`.
    pub fn touch_all(&mut self) {
        for index in 0..self.content.len() {
            self.content.get_index_mut(index);
        }
    }

    /// Recomputes `connects_previous`/`previous_is_text_message` for a
    /// single index, applying the update only where it actually changed.
    /// Both only depend on the *previous* neighbor.
    fn refresh_previous_linkage(&mut self, index: usize) {
        let previous = index
            .checked_sub(1)
            .and_then(|p| self.content.get_index(p))
            .cloned();

        let Some(current) = self.content.get_index(index) else {
            return;
        };

        let connection = current.compute_connection(previous.as_deref());
        let prev_is_text = current.compute_prev_is_event(previous.as_deref());

        if connection.is_none() && prev_is_text.is_none() {
            return;
        }

        if let Some(item) = self.content.get_index_mut(index) {
            let item = Arc::make_mut(item);
            if let Some(connects_before) = connection {
                item.set_connection(connects_before);
            }
            if let Some(prev_is_text) = prev_is_text {
                item.set_previous_is_event(prev_is_text);
            }
        }
    }

    /// After a structural change at `index` (insert/remove/push/pop),
    /// refreshes `index` and the item right after it -- since the linkage
    /// only ever looks backward, those are the only two items whose
    /// `previous` neighbor could possibly have changed.
    fn refresh_previous_linkage_near(&mut self, index: usize) {
        self.refresh_previous_linkage(index);
        self.refresh_previous_linkage(index + 1);
    }

    /// Recomputes the previous-neighbor linkage for every item -- used
    /// after bulk operations (initial load, reset) where structure changed
    /// too broadly to target specific indices.
    fn refresh_all_previous_linkage(&mut self) {
        for index in 0..self.content.len() {
            self.refresh_previous_linkage(index);
        }
    }

    /// Index of the nearest `DateDivider` at or before `index`, if any.
    fn nearest_datedivider_at_or_before(&self, index: usize) -> Option<usize> {
        (0..=index).rev().find(|&i| {
            self.content
                .get_index(i)
                .is_some_and(|item| item.is_date_divider())
        })
    }

    /// Index of the nearest `DateDivider` strictly after `index`, if any.
    fn nearest_datedivider_after(&self, index: usize) -> Option<usize> {
        ((index + 1)..self.content.len()).find(|&i| {
            self.content
                .get_index(i)
                .is_some_and(|item| item.is_date_divider())
        })
    }

    /// Recomputes `depends_on_system_messages` for the `DateDivider` at
    /// `divider_index`, from a snapshot of the items after it, up to (not
    /// including) the next divider -- the only range it depends on.
    /// Snapshotting first avoids borrowing `self.content` both mutably and
    /// immutably at once.
    fn recompute_datedivider(&mut self, divider_index: usize) {
        let mut rest = Vec::new();
        for i in (divider_index + 1)..self.content.len() {
            let Some(item) = self.content.get_index(i) else {
                break;
            };
            if item.is_date_divider() {
                break;
            }
            rest.push(item.clone());
        }

        if let Some(item) = self.content.get_index_mut(divider_index) {
            Arc::make_mut(item).recompute_datedivider_types(rest.iter().map(Arc::as_ref));
        }
    }

    /// After a structural change at `index` (insert/remove), refreshes the
    /// nearest `DateDivider` at or before `index` -- since a divider's scan
    /// only ever looks forward from its own position, that's the only one
    /// whose range could reach `index`. If a `DateDivider` landed exactly at
    /// `index` (freshly inserted, or shifted there by a removal), it's a
    /// boundary the divider after it now depends on, so that one is
    /// refreshed too.
    fn refresh_datedivider_near(&mut self, index: usize) {
        let Some(divider_index) = self.nearest_datedivider_at_or_before(index) else {
            return;
        };
        self.recompute_datedivider(divider_index);

        if divider_index == index
            && let Some(next_index) = self.nearest_datedivider_after(index)
        {
            self.recompute_datedivider(next_index);
        }
    }

    /// Recomputes `depends_on_system_messages` for every `DateDivider` --
    /// used after bulk operations (initial load, reset) where structure
    /// changed too broadly to target specific indices.
    fn refresh_all_datedividers(&mut self) {
        for index in 0..self.content.len() {
            if self
                .content
                .get_index(index)
                .is_some_and(|item| item.is_date_divider())
            {
                self.recompute_datedivider(index);
            }
        }
    }

    pub fn set_scroll_finished(&mut self, direction: PaginationDirection, finished: bool) {
        match direction {
            PaginationDirection::Forward => {
                self.reached_bottom = finished;
                self.loading_bottom = false;
            }
            PaginationDirection::Backward => {
                self.reached_top = finished;
                self.loading_top = false;
            }
        }
    }

    /// The index of the bottom-most item that is an actual message,
    /// skipping trailing non-message items (e.g. the read marker) --
    /// used to seed initial focus so `j`/`k` start on a real message.
    fn last_message_index(&self) -> Option<usize> {
        let mut index = self.content.len().checked_sub(1)?;
        loop {
            if self
                .content
                .index_map()
                .get_index(index)
                .is_some_and(|(_, item)| item.message_event().is_some())
            {
                return Some(index);
            }
            index = index.checked_sub(1)?;
        }
    }

    fn pagination_task(
        &mut self,
        direction: PaginationDirection,
        amount: u16,
    ) -> Option<TimelineAction> {
        let already_loading = match direction {
            PaginationDirection::Forward => self.loading_bottom,
            PaginationDirection::Backward => self.loading_top,
        };

        if already_loading {
            return None;
        }

        match direction {
            PaginationDirection::Forward => self.loading_bottom = true,
            PaginationDirection::Backward => self.loading_top = true,
        };

        tracing::trace!("Scrolling to edge: direction={:?}", direction);

        let timeline = self.timeline.clone()?;
        Some(TimelineAction::Scroll {
            direction,
            task: Task::future(async move {
                match direction {
                    PaginationDirection::Forward => timeline.paginate_forwards(amount).await,
                    PaginationDirection::Backward => timeline.paginate_backwards(amount).await,
                }
                .map_err(|e| {
                    tracing::error!("Failed to paginate: {}", e);
                })
                // Treat failure to paginate like hitting the end
                .unwrap_or(true)
            }),
        })
    }

    pub fn load_timeline(
        &mut self,
        timeline: Arc<Timeline>,
        initial: Arc<IndexMap<String, Arc<TimelineItem>>>,
        power_levels: Arc<RoomPowerLevels>,
    ) -> Option<TimelineAction> {
        let length = initial.len();
        tracing::debug!(
            "ChatTimeline for room {} loaded with {} initial items",
            self.room_id,
            length
        );
        self.timeline = Some(timeline);
        self.power_levels = power_levels;
        self.recalculate_with_power_levels();

        self.content = list::Content::with_items((*initial).clone());
        self.refresh_all_previous_linkage();
        self.refresh_all_datedividers();
        self.messages_version += 1;

        if length < 50 {
            // Not enough content yet to meaningfully snap to
            // `scroll_position` -- deferred to `Diffs`, once the
            // top-up below actually lands.
            return self.pagination_task(PaginationDirection::Backward, 50 - length as u16);
        }

        self.restored_scroll = true;
        Some(TimelineAction::Run(self.restore_scroll_task()))
    }
}

impl IcedWidget<TimelineMessage, TimelineAction> for ChatTimeline {
    fn update(&mut self, message: TimelineMessage) -> Option<TimelineAction> {
        match message {
            TimelineMessage::PinnedEventIds(pinned_ids) => {
                for event_id in self
                    .previous_pinned_event_ids
                    .symmetric_difference(&pinned_ids)
                {
                    let Some(id) = self.content.index_map().values().find_map(|item| {
                        if item.event_id_ref() == Some(event_id) {
                            Some(item.id.clone())
                        } else {
                            None
                        }
                    }) else {
                        continue;
                    };

                    if let Some(item) = self.content.get_mut(&id) {
                        Arc::make_mut(item).set_pinned(pinned_ids.contains(event_id));
                    }
                }

                self.previous_pinned_event_ids = pinned_ids;

                None
            }
            TimelineMessage::ContextMenu(menu) => Some(TimelineAction::ContextMenu(menu)),
            TimelineMessage::HelpHover(help_key) => Some(TimelineAction::HelpHover(help_key)),
            TimelineMessage::ButtonsHovered { id, hovered } => {
                if self.hovered_item_id.as_deref() != Some(id.as_str()) {
                    return None;
                }

                self.buttons_hovered = hovered;

                if !hovered {
                    let row_still_hovered =
                        self.content.get(&id).is_some_and(|item| item.is_hovered());

                    if !row_still_hovered {
                        self.hovered_item_id = None;
                    }
                }
                None
            }
            TimelineMessage::Item { id, message } => {
                let Some(timeline) = &self.timeline else {
                    return None;
                };

                let Some(item) = self.content.get_mut(&id) else {
                    tracing::warn!("No item found for id {}", id);
                    return None;
                };

                let res = match Arc::make_mut(item).update(message)? {
                    TimelineItemAction::LinkClick(link) => {
                        tracing::trace!("Link click: {}", link);
                        Some(TimelineAction::Run(Task::future(async move {
                            if let Err(e) = open::that(&link) {
                                tracing::error!("Failed to open link: {e}");
                            }
                        })))
                    }
                    TimelineItemAction::Mention(mention) => {
                        tracing::trace!("Mention click: {:?}", mention);
                        None
                    }
                    TimelineItemAction::OpenDeleteMenu(event_id) => {
                        item.message_event().map(|event| {
                            TimelineAction::OpenModifyItem(ModifyItem::delete(
                                timeline.clone(),
                                event_id,
                                event,
                            ))
                        })
                    }
                    TimelineItemAction::OpenPinMenu {
                        event_id,
                        is_pinned,
                    } => item.message_event().map(|event| {
                        TimelineAction::OpenModifyItem(ModifyItem::pin(
                            timeline.clone(),
                            event_id,
                            event,
                            is_pinned,
                        ))
                    }),
                    TimelineItemAction::HelpHover(help) => Some(TimelineAction::HelpHover(help)),
                    TimelineItemAction::OpenProfileOverlay {
                        room_id,
                        user_id,
                        bounds,
                    } => Some(TimelineAction::OpenProfileOverlay {
                        room_id,
                        user_id,
                        bounds: Rectangle {
                            y: bounds.y - self.scrolled_from_top,
                            ..bounds
                        },
                    }),
                    TimelineItemAction::HoverChanged(hovered) => {
                        if hovered {
                            if let Some(old_id) = &self.hovered_item_id
                                && old_id != &id
                                && let Some(old_item) = self.content.get_mut(old_id)
                            {
                                Arc::make_mut(old_item).set_is_hovered(false);
                            }

                            if self.hovered_item_id.as_deref() != Some(&id) {
                                self.buttons_hovered = false;
                            }
                            self.hovered_item_id = Some(id.clone());
                        } else if self.hovered_item_id.as_deref() == Some(&id)
                            && !self.buttons_hovered
                        {
                            self.hovered_item_id = None;
                        }
                        self.messages_version += 1;
                        None
                    }
                    TimelineItemAction::MessageEventBounds(bounds) => {
                        self.message_event_bounds.insert(id.clone(), bounds);
                        None
                    }
                    TimelineItemAction::Update => {
                        self.messages_version += 1;
                        None
                    }
                    TimelineItemAction::NeedsMedia(needs_media) => {
                        match &needs_media {
                            NeedsMedia::Avatar { uri } => {
                                self.avatar_states_for_hash.insert(uri.clone());
                            }
                            NeedsMedia::Thumbnail { key, .. } => {
                                self.thumbnail_states_for_hash.insert(key.clone());
                            }
                            NeedsMedia::Video { source, .. } => {
                                self.video_states_for_hash.insert(source.unique_key());
                            }
                        }
                        Some(TimelineAction::NeedsMedia(needs_media))
                    }
                    TimelineItemAction::SetIsReplyingTo { message, event_id } => {
                        if let Some(prev_replying_to_id) = self.replying_to.as_ref()
                            && let Some(item) = self.content.get_mut(prev_replying_to_id)
                        {
                            Arc::make_mut(item).remove_replying();
                        }
                        self.replying_to = Some(id.clone());
                        Some(TimelineAction::SetIsReplyingTo { message, event_id })
                    }
                };

                if self.hovered_item_id.as_ref() == Some(&id)
                    && self.buttons_hovered
                    && let Some(item) = self.content.get_mut(&id)
                {
                    Arc::make_mut(item).set_is_hovered(true);
                }

                res
            }
            TimelineMessage::Loaded {
                timeline,
                initial,
                power_levels,
            } => {
                let length = initial.len();
                tracing::debug!(
                    "ChatTimeline for room {} loaded with {} initial items",
                    self.room_id,
                    length
                );
                self.timeline = Some(timeline);
                self.power_levels = power_levels;
                self.recalculate_with_power_levels();

                self.content = list::Content::with_items((*initial).clone());
                self.refresh_all_previous_linkage();
                self.refresh_all_datedividers();
                self.messages_version += 1;

                if length < 50 {
                    // Not enough content yet to meaningfully snap to
                    // `scroll_position` -- deferred to `Diffs`, once the
                    // top-up below actually lands.
                    return self.pagination_task(PaginationDirection::Backward, 50 - length as u16);
                }

                self.restored_scroll = true;
                Some(TimelineAction::Run(self.restore_scroll_task()))
            }
            TimelineMessage::Diffs(diffs) => {
                tracing::debug!(
                    "ChatTimeline for room {} applying {} diff(s), {} messages before",
                    self.room_id,
                    diffs.len(),
                    self.content.len()
                );

                let room_id = self.room_id.clone();
                let state = self.state.clone();

                for diff in diffs {
                    let diff = diff.map(|m| {
                        (
                            m.unique_id().0.clone(),
                            Arc::new(m.convert(
                                &state,
                                room_id.clone(),
                                &self.previous_pinned_event_ids,
                            )),
                        )
                    });

                    match diff {
                        VectorDiff::Append { values } => {
                            for (key, value) in values {
                                // The diff already hands us the value, so no
                                // search is needed -- it can only affect a
                                // `DateDivider` if it's one itself (needing
                                // its own first scan); it can never reach
                                // back into an earlier divider's range.
                                let is_new_divider = value.is_date_divider();
                                self.content.push(key, value);
                                // Last item only: appending never has
                                // anything after it whose `previous` could
                                // change.
                                self.refresh_previous_linkage(self.content.len() - 1);
                                if is_new_divider {
                                    self.recompute_datedivider(self.content.len() - 1);
                                }
                            }
                        }
                        VectorDiff::Clear => self.content = list::Content::new(),
                        VectorDiff::Insert {
                            index,
                            value: (key, value),
                        } => {
                            self.content.insert(index, key, value);
                            self.refresh_previous_linkage_near(index);
                            self.refresh_datedivider_near(index);
                        }
                        // Removing from the end can't change any remaining
                        // item's `previous` neighbor. The diff doesn't carry
                        // the removed value, though, so unlike `Append` we
                        // can't shortcut straight to "was it a divider" --
                        // do the general search instead.
                        VectorDiff::PopBack => {
                            if !self.content.is_empty() {
                                self.content.remove(self.content.len() - 1);
                                self.refresh_datedivider_near(self.content.len());
                            }
                        }
                        VectorDiff::PopFront => {
                            if !self.content.is_empty() {
                                self.content.remove(0);
                                self.refresh_previous_linkage(0);
                                self.refresh_datedivider_near(0);
                            }
                        }
                        VectorDiff::PushBack {
                            value: (key, value),
                        } => {
                            let is_new_divider = value.is_date_divider();
                            self.content.push(key, value);
                            self.refresh_previous_linkage(self.content.len() - 1);
                            if is_new_divider {
                                self.recompute_datedivider(self.content.len() - 1);
                            }
                        }
                        VectorDiff::PushFront {
                            value: (key, value),
                        } => {
                            self.content.insert(0, key, value);
                            self.refresh_previous_linkage_near(0);
                            self.refresh_datedivider_near(0);
                        }
                        VectorDiff::Remove { index } => {
                            self.content.remove(index);
                            self.refresh_previous_linkage(index);
                            self.refresh_datedivider_near(index);
                        }
                        VectorDiff::Reset { values } => {
                            self.content = list::Content::with_items(values.into_iter().collect());
                            self.refresh_all_previous_linkage();
                            self.refresh_all_datedividers();
                        }
                        // Matrix timeline updates (edits, reactions,
                        // redactions-in-place) never change an item's
                        // sender, timestamp, or whether it's a message at
                        // all -- the only things the linkage depends on --
                        // and don't shift any other item's position either,
                        // so nothing here can ever need recomputing.
                        VectorDiff::Set {
                            index,
                            value: (_, value),
                        } => {
                            if let Some(slot) = self.content.get_index_mut(index) {
                                *slot = value;
                            }
                        }
                        // Truncating only removes from the end, which can't
                        // change any remaining item's `previous` neighbor or
                        // any `DateDivider`'s dependencies.
                        VectorDiff::Truncate { length } => {
                            while self.content.len() > length {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                    }
                }

                self.messages_version += 1;

                if !self.restored_scroll && !self.content.is_empty() {
                    self.restored_scroll = true;
                    Some(TimelineAction::Run(self.restore_scroll_task()))
                } else {
                    None
                }
            }
            TimelineMessage::Scroll {
                direction,
                position,
            } => {
                self.scroll_position = position;

                direction.and_then(|d| self.pagination_task(d, 30))
            }
            TimelineMessage::ScrolledFromTop(offset) => {
                self.scrolled_from_top = offset;
                None
            }
            TimelineMessage::ChatAreaBounds(bounds) => {
                self.tile_bounds = bounds;
                None
            }
            TimelineMessage::RestoreScrollPosition => {
                if self.content.is_empty() {
                    // Nothing loaded yet -- `Loaded`/`Diffs` will snap once
                    // there's actually content to snap against.
                    return None;
                }

                self.restored_scroll = true;
                Some(TimelineAction::Run(self.restore_scroll_task()))
            }
            TimelineMessage::KeyboardEvent(event) => {
                if let iced::keyboard::Event::KeyPressed { key, .. } = event {
                    let length = self.content.len();

                    if key == iced::keyboard::Key::Character("k".into()) {
                        let mut new_focus_index = match self
                            .focused_message_id
                            .as_ref()
                            .and_then(|id| self.content.index_map().get_index_of(id))
                        {
                            Some(focus) => {
                                if focus == 0 {
                                    return None;
                                } else {
                                    focus.saturating_sub(1)
                                }
                            }
                            None => self.last_message_index()?,
                        };

                        while let Some((id, item)) =
                            self.content.index_map().get_index(new_focus_index)
                        {
                            if item.message_event().is_some() {
                                tracing::debug!("Focusing message on index {new_focus_index}");

                                // Mark the previously and newly focused message to be updated
                                // by calling the get_mut method. The messages will then be
                                // internally marked as updated and be rerendered.
                                if let Some(old_id) = self.focused_message_id.take() {
                                    self.content.get_mut(&old_id);
                                }
                                self.content.get_mut(id);

                                self.focused_message_id = Some(id.clone());

                                let task = operate(sweeten::widget::list::FindItemBounds::new(
                                    Some(LIST_ID),
                                    new_focus_index,
                                ))
                                .map(TimelineMessage::ScrollTo);
                                return Some(TimelineAction::Perform(task));
                            }
                            if new_focus_index == 0 {
                                break;
                            }
                            new_focus_index -= 1;
                        }
                    }

                    if key == iced::keyboard::Key::Character("j".into()) {
                        let mut new_focus_index = match self
                            .focused_message_id
                            .as_ref()
                            .and_then(|id| self.content.index_map().get_index_of(id))
                        {
                            Some(focus) if focus >= length - 1 => {
                                return None;
                            }
                            Some(focus) => focus + 1,
                            None => self.last_message_index()?,
                        };

                        while let Some((id, item)) =
                            self.content.index_map().get_index(new_focus_index)
                        {
                            if item.message_event().is_some() {
                                tracing::debug!("Focusing message on index {new_focus_index}");

                                // Mark the previously and newly focused message to be updated
                                // by calling the get_mut method. The messages will then be
                                // internally marked as updated and be rerendered.
                                if let Some(old_id) = self.focused_message_id.take() {
                                    self.content.get_mut(&old_id);
                                }
                                self.content.get_mut(id);

                                self.focused_message_id = Some(id.clone());

                                let task = operate(sweeten::widget::list::FindItemBounds::new(
                                    Some(LIST_ID),
                                    new_focus_index,
                                ))
                                .map(TimelineMessage::ScrollTo);
                                return Some(TimelineAction::Perform(task));
                            }
                            if new_focus_index >= length - 1 {
                                break;
                            }
                            new_focus_index += 1;
                        }
                        None
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            TimelineMessage::ScrollTo(opt_rect) => {
                let rect = opt_rect?;
                tracing::debug!("Item to scroll to is at y: {}", rect.y);
                Some(TimelineAction::Run(scroll_to::<()>(
                    SCROLLABLE_ID,
                    AbsoluteOffset {
                        x: rect.x,
                        y: rect.y,
                    },
                    w::operation::Animation::Smooth,
                )))
            }
            TimelineMessage::None => None,
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, TimelineMessage> {
        let Some(timeline) = self.timeline.clone() else {
            return Space::new().width(Fill).height(Fill).into();
        };

        let reached_top = self.reached_top;
        let reached_bottom = self.reached_bottom;
        let loading_top = self.loading_top;
        let loading_bottom = self.loading_bottom;

        let focused_id = Rc::new(self.focused_message_id.clone());

        let mut stack = Stack::new()
            .push(
                w::container(track_scroll(
                    themed_scrollable(
                        w::container(
                            list(self.content.clone(), move |index, id, item| {
                                let is_focused =
                                    focused_id.as_ref().as_ref().is_some_and(|i| *i == id);
                                w::lazy(
                                    (item.clone(), is_focused, help_state),
                                    move |(item, is_focused, help_state)| {
                                        let id = id.clone();
                                        item.view(theme, structure, *is_focused, *help_state, index)
                                            .map(move |msg| TimelineMessage::Item {
                                                id: id.clone(),
                                                message: msg,
                                            })
                                    },
                                )
                                .into()
                            })
                            .id(LIST_ID),
                        )
                        .padding(padding::bottom(structure.gap * 3.0))
                        .width(Fill)
                        .height(Fill)
                        .align_bottom(Fill),
                        theme,
                        structure,
                    )
                    .spacing(structure.small_gap)
                    .width(Fill)
                    .anchor_bottom()
                    .smooth_scroll(true)
                    .on_scroll(move |scroll| {
                        let viewport = scroll.destination();

                        let max_offset =
                            (viewport.content.height - viewport.bounds.height).max(0.0);
                        let offset = viewport.absolute_offset().y;
                        let position = viewport.relative_offset();

                        let mut direction = None;
                        if !reached_top
                            && !loading_top
                            && max_offset - offset <= structure.chat.icon_size * 6.0
                        {
                            direction = Some(PaginationDirection::Backward);
                        } else if !reached_bottom
                            && !loading_bottom
                            && offset <= structure.chat.icon_size * 6.0
                        {
                            direction = Some(PaginationDirection::Forward);
                        }

                        TimelineMessage::Scroll {
                            direction,
                            position,
                        }
                    })
                    .id(SCROLLABLE_ID),
                    SCROLLABLE_ID,
                    TimelineMessage::ScrolledFromTop,
                ))
                .width(Fill)
                .align_bottom(Fill),
            )
            .height(Fill);

        if let Some(hovered_item_id) = self.hovered_item_id.clone()
            && let Some(bounds) = self.message_event_bounds.get(&hovered_item_id).cloned()
            && let Some((
                Some(event_id),
                (is_own, is_editable, can_be_replied_to),
                is_pinned,
                Some(message),
            )) = self.content.get(&hovered_item_id).map(|item| {
                (
                    item.event_id(),
                    item.booleans(),
                    item.is_pinned(),
                    item.message_event(),
                )
            })
        {
            let can_edit = is_editable && self.user_can_send;
            let can_reply = can_be_replied_to && self.user_can_send;
            let can_pin = self.user_can_pin;
            let can_redact = if is_own {
                self.user_can_redact_own
            } else {
                self.user_can_redact_other
            };

            let scrolled_from_top = self.scrolled_from_top;
            let tile_bounds = self.tile_bounds;

            let menu_timeline = timeline.clone();
            let menu_room_id = self.room_id.clone();
            let menu_event_id = event_id.clone();
            let menu_item_id = hovered_item_id.clone();
            let menu_event = message.clone();

            let context_menu_catcher: Element<'static, ContextMenu> = w::float(
                sweeten::widget::mouse_area(Space::new().width(bounds.width).height(bounds.height))
                    .on_right_press(move |position, _| ContextMenu {
                        // `position` is local to this catcher's own bounds (per
                        // sweeten's `position_in`), so shift it by the row's
                        // absolute position to land in the same window-space
                        // coordinates as `bounds`.
                        position: Point::new(
                            bounds.x + position.x,
                            bounds.y - scrolled_from_top + position.y,
                        ),
                        kind: ContextMenuKind::Message {
                            timeline: menu_timeline.clone(),
                            event: menu_event.clone(),
                            room_id: menu_room_id.clone(),
                            event_id: menu_event_id.clone(),
                            item_id: menu_item_id.clone(),

                            is_pinned,

                            can_edit,
                            can_reply,
                            can_pin,
                            can_redact,
                        },
                    }),
            )
            .translate(move |own_bounds, _| {
                let target_x = bounds.x;
                let target_y = bounds.y - scrolled_from_top;

                Vector::new(target_x - own_bounds.x, target_y - own_bounds.y)
            })
            .into();

            stack = stack
                .push(context_menu_catcher.map(TimelineMessage::ContextMenu))
                .push(
                    w::float(render_timeline_item_buttons(
                        structure,
                        theme,
                        event_id.clone(),
                        hovered_item_id.clone(),
                        is_pinned,
                        can_edit,
                        can_reply,
                        can_pin,
                        can_redact,
                    ))
                    .translate(move |own_bounds, _| {
                        let target_x = bounds.x + bounds.width - own_bounds.width;
                        let target_y = bounds.y - scrolled_from_top - own_bounds.height / 2.0;

                        // Never let the buttons render outside the chat tile --
                        // clamp the target into `tile_bounds` before converting
                        // it into an offset from the float's own position.
                        let min_x = tile_bounds.x;
                        let max_x =
                            (tile_bounds.x + tile_bounds.width - own_bounds.width).max(min_x);
                        let min_y = tile_bounds.y;
                        let max_y =
                            (tile_bounds.y + tile_bounds.height - own_bounds.height).max(min_y);

                        let clamped_x = target_x.clamp(min_x, max_x);
                        let clamped_y = target_y.clamp(min_y, max_y);

                        Vector::new(clamped_x - own_bounds.x, clamped_y - own_bounds.y)
                    }),
                );
        }

        track_bounds(stack, TimelineMessage::ChatAreaBounds).into()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_timeline_item_buttons(
    structure: Structure,
    theme: Theme,
    event_id: OwnedEventId,
    item_id: String,
    is_pinned: bool,
    can_edit: bool,
    can_reply: bool,
    can_pin: bool,
    can_redact: bool,
) -> Element<'static, TimelineMessage> {
    let mut buttons = Vec::new();

    let button_size = structure.chat.small_icon_size;
    let convert_message = |message| TimelineMessage::Item {
        id: item_id.clone(),
        message,
    };

    let button = move |icon: &'static str,
                       message: TimelineItemMessage,
                       hover_bg: DePlaceColor,
                       color: DePlaceColor,
                       hover_color: DePlaceColor| {
        w::button(phosphor_icon(icon, button_size))
            .padding(structure.small_gap / 2.0)
            .on_press(convert_message(message))
            .style(move |_: &IcedTheme, status| ButtonStyle {
                background: if status.active() {
                    Some(hover_bg.into())
                } else {
                    None
                },
                text_color: if status.active() { hover_color } else { color }.into(),
                border: border::rounded(structure.semi_border_radius()),
                ..Default::default()
            })
            .into()
    };

    if can_edit {
        buttons.push(button(
            icons::pencil_simple::BOLD,
            TimelineItemMessage::SetIsEditing(true),
            theme.solid_hover_bg,
            theme.text.dim,
            theme.text.normal,
        ));
    }

    if can_reply {
        buttons.push(button(
            icons::arrow_bend_up_left::BOLD,
            TimelineItemMessage::SetIsReplyingTo(event_id.clone()),
            theme.solid_hover_bg,
            theme.text.dim,
            theme.text.normal,
        ));
    }

    if can_pin {
        buttons.push(button(
            if is_pinned {
                icons::push_pin_slash::BOLD
            } else {
                icons::push_pin::BOLD
            },
            TimelineItemMessage::OpenPinMenu {
                event_id: event_id.clone(),
                is_pinned,
            },
            theme.colors.yellow,
            theme.colors.yellow,
            theme.solid_bg,
        ));
    }

    if can_redact {
        buttons.push(button(
            icons::trash::BOLD,
            TimelineItemMessage::OpenDeleteMenu(event_id.clone()),
            theme.colors.error,
            theme.colors.error,
            theme.solid_bg,
        ));
    }

    if !buttons.is_empty() {
        w::mouse_area(
            w::container(w::Row::with_children(buttons).spacing(structure.small_gap / 2.0))
                .style(move |_| ContainerStyle {
                    background: Some(theme.solid_bg.into()),
                    border: Border {
                        color: theme.border.into(),
                        width: structure.border_thickness,
                        radius: structure.semi_border_radius().into(),
                    },
                    ..Default::default()
                })
                .padding(structure.small_gap / 2.0),
        )
        .on_enter(TimelineMessage::ButtonsHovered {
            id: item_id.clone(),
            hovered: true,
        })
        .on_exit(TimelineMessage::ButtonsHovered {
            id: item_id,
            hovered: false,
        })
        .into()
    } else {
        Space::new().into()
    }
}
