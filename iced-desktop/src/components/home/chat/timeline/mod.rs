use std::collections::{BTreeSet, HashMap};

use deplace_core::PaginationDirection;
use iced::Vector;
use macros::iced_cache;
use matrix_sdk::{
    media::UniqueKey,
    ruma::{
        events::{MessageLikeEventType, StateEventType, room::power_levels::RoomPowerLevels},
        room_version_rules::AuthorizationRules,
    },
};
use matrix_sdk_ui::{Timeline, eyeball_im::VectorDiff, timeline::TimelineItem as UiTimelineItem};
use messages::{TimelineItem, TimelineItemAction, TimelineItemMessage};
use sweeten::widget::list;

use phosphor_svgs::icon as icons;

use crate::{
    common::*,
    components::{track_bounds::track_bounds, track_scroll::track_scroll},
};

pub use messages::ToTimelineItem;
mod messages;

#[derive(Debug, Clone)]
pub enum TimelineMessage {
    Item {
        id: String,
        message: TimelineItemMessage,
    },
    Loaded {
        timeline: Arc<Timeline>,
        initial: Arc<Vec<Arc<TimelineItem>>>,
        power_levels: Arc<RoomPowerLevels>,
    },
    Diffs(Vec<VectorDiff<Arc<UiTimelineItem>>>),
    SetReplying(OwnedEventId),
    None,
    Scroll {
        direction: PaginationDirection,
    },
    ScrolledFromTop(f32),
    ButtonsHovered {
        id: String,
        hovered: bool,
    },
    ChatAreaBounds(Rectangle),
}

pub enum TimelineAction {
    NeedsMedia(NeedsMedia),
    SetIsReplyingTo(OwnedEventId),
    Run(Task<()>),
    Scroll {
        direction: PaginationDirection,
        task: Task<bool>,
    },
}

#[iced_cache(Clone)]
pub struct ChatTimeline {
    state: AppState,
    timeline: Option<Arc<Timeline>>,

    // TODO: Add logic to update this
    power_levels: Arc<RoomPowerLevels>,
    user_can_send: bool,
    user_can_pin: bool,
    user_can_redact_own: bool,
    user_can_redact_other: bool,

    scrolled_from_top: f32,

    #[hash]
    room_id: OwnedRoomId,
    own_user_id: OwnedUserId,

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

    message_event_bounds: HashMap<String, Rectangle>,
    tile_bounds: Rectangle,

    content: list::Content<Arc<TimelineItem>>,
    id_index: Arc<HashMap<String, usize>>,
    #[hash]
    messages_version: u64,
}

impl ExtraHash for ChatTimeline {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.scrolled_from_top.to_bits().hash(state);
        self.tile_bounds.x.to_bits().hash(state);
        self.tile_bounds.y.to_bits().hash(state);
        self.tile_bounds.width.to_bits().hash(state);
        self.tile_bounds.height.to_bits().hash(state);
    }
}

impl ChatTimeline {
    pub fn new(room: &Room, state: &AppState) -> Self {
        let power_levels = Arc::new(RoomPowerLevels::new(
            matrix_sdk::ruma::events::room::power_levels::RoomPowerLevelsSource::None,
            &AuthorizationRules::V12,
            [],
        ));
        let own_user_id = room.own_user_id().to_owned();

        let mut timeline = Self {
            state: state.clone(),
            timeline: None,

            power_levels,
            user_can_send: false,
            user_can_pin: false,
            user_can_redact_own: false,
            user_can_redact_other: false,

            scrolled_from_top: 0.0,

            room_id: room.room_id().to_owned(),
            own_user_id,

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

            message_event_bounds: HashMap::new(),
            // No-op until the first real measurement comes in via `ChatAreaBounds`.
            tile_bounds: Rectangle {
                x: -1_000_000.0,
                y: -1_000_000.0,
                width: 2_000_000.0,
                height: 2_000_000.0,
            },

            content: list::Content::default(),
            id_index: Arc::new(HashMap::new()),
            messages_version: 0,
        };
        timeline.recalculate_with_power_levels();
        timeline
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
            self.content.get_mut(index);
        }
    }

    /// Recomputes `connects_previous`/`previous_is_text_message` for a
    /// single index, applying the update only where it actually changed.
    /// Both only depend on the *previous* neighbor.
    fn refresh_previous_linkage(&mut self, index: usize) {
        let previous = index
            .checked_sub(1)
            .and_then(|p| self.content.get(p))
            .cloned();

        let Some(current) = self.content.get(index) else {
            return;
        };

        let connection = current.compute_connection(previous.as_deref());
        let prev_is_text = current.compute_prev_is_event(previous.as_deref());

        if connection.is_none() && prev_is_text.is_none() {
            return;
        }

        if let Some(item) = self.content.get_mut(index) {
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
}

const SCROLLABLE_ID: iced::widget::Id = iced::widget::Id::new("timeline-scrollable");

impl IcedWidget<TimelineMessage, TimelineAction> for ChatTimeline {
    fn update(&mut self, message: TimelineMessage) -> Option<TimelineAction> {
        match message {
            TimelineMessage::ButtonsHovered { id, hovered } => {
                if self.hovered_item_id.as_deref() != Some(id.as_str()) {
                    return None;
                }

                self.buttons_hovered = hovered;

                if !hovered {
                    let row_still_hovered = self
                        .id_index
                        .get(&id)
                        .and_then(|ix| self.content.get(*ix))
                        .is_some_and(|item| item.is_hovered());

                    if !row_still_hovered {
                        self.hovered_item_id = None;
                    }
                }
            }
            TimelineMessage::Item { id, message } => {
                let Some(&index) = self.id_index.get(&id) else {
                    tracing::warn!("No item found for id {}", id);
                    return None;
                };
                let Some(item) = self.content.get_mut(index) else {
                    tracing::warn!("No item found for id {}", id);
                    return None;
                };
                let res = if let Some(action) = Arc::make_mut(item).update(message) {
                    match action {
                        TimelineItemAction::HoverChanged(hovered) => {
                            if hovered {
                                if let Some(old_id) = &self.hovered_item_id
                                    && old_id != &id
                                    && let Some(&old_index) = self.id_index.get(old_id)
                                    && let Some(old_item) = self.content.get_mut(old_index)
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
                        TimelineItemAction::SetIsReplyingTo(event_id) => {
                            Some(TimelineAction::SetIsReplyingTo(event_id.clone()))
                        }
                    }
                } else {
                    None
                };

                if self.hovered_item_id == Some(id)
                    && self.buttons_hovered
                    && let Some(item) = self.content.get_mut(index)
                {
                    Arc::make_mut(item).set_is_hovered(true);
                }

                return res;
            }
            TimelineMessage::Loaded {
                timeline,
                initial,
                power_levels,
            } => {
                tracing::debug!(
                    "ChatTimeline for room {} loaded with {} initial items",
                    self.room_id,
                    initial.len()
                );
                self.timeline = Some(timeline);
                self.power_levels = power_levels;
                self.recalculate_with_power_levels();

                self.id_index = Arc::new(
                    initial
                        .iter()
                        .enumerate()
                        .map(|(index, item)| (item.id.clone(), index))
                        .collect(),
                );
                self.content = list::Content::with_items((*initial).clone());
                self.refresh_all_previous_linkage();
                self.messages_version += 1;

                if initial.len() < 50 {
                    return self
                        .pagination_task(PaginationDirection::Forward, 50 - initial.len() as u16);
                }
            }
            TimelineMessage::Diffs(diffs) => {
                tracing::debug!(
                    "ChatTimeline for room {} applying {} diff(s), {} messages before",
                    self.room_id,
                    diffs.len(),
                    self.content.len()
                );

                let avatar_cache = self.state.avatar_cache().clone();
                let thumbnail_cache = self.state.thumbnail_cache().clone();
                let video_cache = self.state.video_cache().clone();
                let room_id = self.room_id.clone();

                for diff in diffs.into_iter().map(|d| {
                    d.map(|m| {
                        Arc::new(m.convert(
                            &avatar_cache,
                            &thumbnail_cache,
                            &video_cache,
                            room_id.clone(),
                        ))
                    })
                }) {
                    match diff {
                        VectorDiff::Append { values } => {
                            for value in values {
                                self.content.push(value);
                                // Last item only: appending never has
                                // anything after it whose `previous` could
                                // change.
                                self.refresh_previous_linkage(self.content.len() - 1);
                            }
                        }
                        VectorDiff::Clear => self.content = list::Content::new(),
                        VectorDiff::Insert { index, value } => {
                            self.content.insert(index, value);
                            self.refresh_previous_linkage_near(index);
                        }
                        // Removing from the end can't change any remaining
                        // item's `previous` neighbor.
                        VectorDiff::PopBack => {
                            if !self.content.is_empty() {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                        VectorDiff::PopFront => {
                            if !self.content.is_empty() {
                                self.content.remove(0);
                                self.refresh_previous_linkage(0);
                            }
                        }
                        VectorDiff::PushBack { value } => {
                            self.content.push(value);
                            self.refresh_previous_linkage(self.content.len() - 1);
                        }
                        VectorDiff::PushFront { value } => {
                            self.content.insert(0, value);
                            self.refresh_previous_linkage_near(0);
                        }
                        VectorDiff::Remove { index } => {
                            self.content.remove(index);
                            self.refresh_previous_linkage(index);
                        }
                        VectorDiff::Reset { values } => {
                            self.content = list::Content::with_items(values.into_iter().collect());
                            self.refresh_all_previous_linkage();
                        }
                        // Matrix timeline updates (edits, reactions,
                        // redactions-in-place) never change an item's
                        // sender, timestamp, or whether it's a message at
                        // all -- the only things the linkage depends on --
                        // and don't shift any other item's position either,
                        // so nothing here can ever need recomputing.
                        VectorDiff::Set { index, value } => {
                            if let Some(slot) = self.content.get_mut(index) {
                                *slot = value;
                            }
                        }
                        // Truncating only removes from the end, which can't
                        // change any remaining item's `previous` neighbor.
                        VectorDiff::Truncate { length } => {
                            while self.content.len() > length {
                                self.content.remove(self.content.len() - 1);
                            }
                        }
                    }
                }

                self.id_index = Arc::new(
                    (0..self.content.len())
                        .filter_map(|index| {
                            self.content.get(index).map(|item| (item.id.clone(), index))
                        })
                        .collect(),
                );
                self.messages_version += 1;
            }
            TimelineMessage::SetReplying(event_id) => {
                return Some(TimelineAction::SetIsReplyingTo(event_id));
            }
            TimelineMessage::Scroll { direction } => {
                return self.pagination_task(direction, 30);
            }
            TimelineMessage::ScrolledFromTop(offset) => {
                self.scrolled_from_top = offset;
            }
            TimelineMessage::ChatAreaBounds(bounds) => {
                self.tile_bounds = bounds;
            }
            TimelineMessage::None => {}
        };

        None
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, TimelineMessage> {
        let reached_top = self.reached_top;
        let reached_bottom = self.reached_bottom;
        let loading_top = self.loading_top;
        let loading_bottom = self.loading_bottom;

        let mut stack = Stack::new()
            .push(
                w::container(track_scroll(
                    themed_scrollable(
                        w::container(list(self.content.clone(), move |_index, item| {
                            let id = item.id.clone();
                            w::lazy(item.clone(), move |item| {
                                let id = id.clone();
                                item.view(theme, structure)
                                    .map(move |msg| TimelineMessage::Item {
                                        id: id.clone(),
                                        message: msg,
                                    })
                            })
                            .into()
                        }))
                        .padding(padding::bottom(structure.gap * 3.0))
                        .width(Fill),
                        theme,
                        structure,
                    )
                    .spacing(structure.small_gap)
                    .width(Fill)
                    .anchor_bottom()
                    .on_scroll(move |viewport| {
                        let max_offset =
                            (viewport.content_bounds().height - viewport.bounds().height).max(0.0);
                        let offset = viewport.absolute_offset().y;

                        if !reached_top
                            && !loading_top
                            && max_offset - offset <= structure.chat.icon_size * 6.0
                        {
                            return TimelineMessage::Scroll {
                                direction: PaginationDirection::Backward,
                            };
                        }
                        if !reached_bottom
                            && !loading_bottom
                            && offset <= structure.chat.icon_size * 6.0
                        {
                            return TimelineMessage::Scroll {
                                direction: PaginationDirection::Forward,
                            };
                        }

                        TimelineMessage::None
                    })
                    .id(SCROLLABLE_ID),
                    SCROLLABLE_ID,
                    TimelineMessage::ScrolledFromTop,
                ))
                .width(Fill)
                .align_bottom(Fill),
            )
            .height(Fill);

        if let Some(hovered_item_id) = &self.hovered_item_id
            && let Some(bounds) = self.message_event_bounds.get(hovered_item_id).cloned()
            && let Some((Some(event_id), (is_own, is_editable, can_be_replied_to))) = self
                .id_index
                .get(hovered_item_id)
                .and_then(|ix| self.content.get(*ix))
                .map(|item| (item.event_id(), item.booleans()))
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

            stack = stack.push(
                w::float(render_timeline_item_buttons(
                    structure,
                    theme,
                    event_id.clone(),
                    hovered_item_id.clone(),
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
                    let max_x = (tile_bounds.x + tile_bounds.width - own_bounds.width).max(min_x);
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
                       hover_bg: Color,
                       color: Color,
                       hover_color: Color| {
        w::button(phosphor_icon(icon, button_size))
            .padding(structure.small_gap / 2.0)
            .on_press(convert_message(message))
            .style(move |_: &IcedTheme, status| ButtonStyle {
                background: if status.active() {
                    Some(hover_bg.into())
                } else {
                    None
                },
                text_color: if status.active() { hover_color } else { color },
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
            icons::push_pin::BOLD,
            TimelineItemMessage::SetIsReplyingTo(event_id.clone()),
            theme.colors.yellow,
            theme.colors.yellow,
            theme.solid_bg,
        ));
    }

    if can_redact {
        buttons.push(button(
            icons::trash::BOLD,
            TimelineItemMessage::SetIsReplyingTo(event_id.clone()),
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
                        color: theme.border,
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
