pub use crate::EMPTY_WIDGET;
pub use crate::components::{
    ColoredRounded, CornerContent, Explainable, GenericState, IcedColorExt, IcedWidget,
    MapElementExt, StatusExt, UI_FONT,
    animated_button::animated_button,
    close_button, corner_badge, elapsed_time,
    help_mode::HelpKey,
    help_mode::HelpView,
    help_mode::{Caption, HelpOverMessage, HelpState, help, help_root},
    on_appear, pan,
    pie_progress::pie_progress,
    profile::{
        MemberRenderExt, ProfileRenderExt, RoomRenderExt, loading_icon, render_avatar,
        render_loading_name, render_name, render_presence_badge, render_unknown_name, text_icon,
        unknown_icon,
    },
    text_input, themed_scrollable,
    tile_background::floating_tile,
    tooltip::{themed_tooltip_content, themed_tooltip_text},
    weighted_text,
};
pub use deplace_core::{
    ProfileLike,
    colors::DePlaceColor,
    get_change,
    helpers::{DisplayString, EventChange, get_current_and_prev},
    member::{Member, store::ProfileStore},
    room::{DePlaceRoom, store::RoomStore},
    settings::Settings,
    state::{AppState, roles::ExtraHash},
    structure::Structure,
    theme::Theme,
};
use iced::advanced::Widget;
use iced::widget::text_editor;
pub use iced::{
    Border, Color, Element, Font,
    Length::Fill,
    Padding, Point, Rectangle, Renderer, Shadow, Size, Subscription, Task, Theme as IcedTheme,
    Widget as _, border,
    font::Weight,
    mouse::{self, Interaction},
    padding,
    widget::{
        self as w, Canvas, Column, MouseArea, Row, Space, Stack, Text,
        button::{self, Style as ButtonStyle},
        canvas,
        container::{self, Container, Style as ContainerStyle},
        image::Handle as ImageHandle,
        svg::{self, Style as SvgStyle},
        text::{self, Style as TextStyle},
    },
    window,
};
pub use indexmap::IndexMap;
pub use matrix_sdk::{
    Client,
    ruma::{
        MxcUri, OwnedEventId, OwnedMxcUri, OwnedRoomId, OwnedUserId, RoomId, UserId,
        presence::PresenceState,
    },
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
pub use std::hash::Hash;
pub use std::sync::Arc;
pub use tokio::sync::watch::Receiver;

/// The widget id of whichever message's inline edit box is currently open, so it can be focused.
pub const EDIT_INPUT_ID: iced::widget::Id = iced::widget::Id::new("message-edit-input");

/// One room's inline-edit state: which message (if any) it's currently showing
/// an edit box for, and the `Content` backing that box. The `Content` is leaked
/// the first time a room starts an edit and reused for any later edit in that
/// same room (just like `ChatInput`'s own persistent content) -- it's only freed
/// via `remove_editing_content` when the room itself is evicted from the chat
/// cache, not when an individual edit ends.
struct RoomEditState {
    event_id: Option<OwnedEventId>,
    content: *mut text_editor::Content,
}

thread_local! {
    static THEME: Cell<Theme> = Cell::new(Theme::default());
    static STRUCTURE: Cell<Structure> = Cell::new(Structure::default());
    static HELP: Cell<HelpState<HelpKey>> = Cell::new(HelpState::default());
    static STYLE_GEN: Cell<u64> = const { Cell::new(0) };
    static EDITING: RefCell<HashMap<OwnedRoomId, RoomEditState>> = RefCell::new(HashMap::new());
    // Bumped whenever any room's editing target changes, so every `lazy`/`lazy_view` boundary in
    // the app (which already keys off `style_gen()`/`help_state()` the same way) picks up the
    // switch between plain text and the inline edit widget for the affected message row.
    static EDIT_GEN: Cell<u64> = const { Cell::new(0) };
}

pub fn theme() -> Theme {
    THEME.get()
}
pub fn structure() -> Structure {
    STRUCTURE.get()
}
pub fn help_state() -> HelpState<HelpKey> {
    HELP.get()
}
pub fn style_gen() -> u64 {
    STYLE_GEN.get()
}

pub fn set_theme(t: Theme) {
    THEME.set(t);
    STYLE_GEN.set(STYLE_GEN.get() + 1);
}

pub fn set_structure(s: Structure) {
    STRUCTURE.set(s);
    STYLE_GEN.set(STYLE_GEN.get() + 1);
}

pub fn set_help_state(h: HelpState<HelpKey>) {
    HELP.set(h);
}

pub fn edit_gen() -> u64 {
    EDIT_GEN.get()
}

/// The raw pointer behind `room_id`'s reused edit `Content`, if it's ever started
/// an edit. Same leaked-`Content` pattern as `ChatInput`'s own `content` field.
pub fn editing_content_ptr(room_id: &OwnedRoomId) -> Option<*mut text_editor::Content> {
    EDITING.with(|m| m.borrow().get(room_id).map(|state| state.content))
}

/// The event id `room_id` is currently showing an inline edit box for, if any.
pub fn editing_event_id(room_id: &OwnedRoomId) -> Option<OwnedEventId> {
    EDITING.with(|m| {
        m.borrow()
            .get(room_id)
            .and_then(|state| state.event_id.clone())
    })
}

/// Switches `room_id`'s edit content over to `event_id`, seeding it with `text`.
/// Safe to call again with a different `event_id` to switch which message is
/// being edited in that room -- it simply overwrites whatever was there.
pub fn start_editing(room_id: OwnedRoomId, event_id: OwnedEventId, text: &str) {
    EDITING.with(|m| {
        let mut map = m.borrow_mut();
        let state = map.entry(room_id).or_insert_with(|| RoomEditState {
            event_id: None,
            content: Box::leak(Box::new(text_editor::Content::new())),
        });

        let content = unsafe { &mut *state.content };
        content.perform(text_editor::Action::SelectAll);
        content.perform(text_editor::Action::Edit(text_editor::Edit::Paste(
            Arc::new(text.to_string()),
        )));
        state.event_id = Some(event_id);
    });
    EDIT_GEN.set(EDIT_GEN.get() + 1);
}

/// Closes whichever inline edit box `room_id` has open, if any. Keeps the room's
/// leaked `Content` around for reuse by a later edit in the same room.
pub fn cancel_editing(room_id: &OwnedRoomId) {
    EDITING.with(|m| {
        if let Some(state) = m.borrow_mut().get_mut(room_id) {
            state.event_id = None;
        }
    });
    EDIT_GEN.set(EDIT_GEN.get() + 1);
}

/// Frees `room_id`'s edit content. Call this when the room itself is evicted
/// from the chat cache -- mirrors freeing `ChatInput`'s own leaked content on
/// that same eviction.
pub fn remove_editing_content(room_id: &OwnedRoomId) {
    let removed = EDITING.with(|m| m.borrow_mut().remove(room_id));
    if let Some(state) = removed {
        unsafe { drop(Box::from_raw(state.content)) };
    }
}

pub fn lazy<'a, Message, Dep, View>(
    dep: Dep,
    view: impl Fn(&Dep) -> View + 'a,
) -> Element<'a, Message>
where
    Message: 'static,
    Dep: Hash + 'a,
    View: Into<Element<'static, Message>> + 'static,
{
    w::lazy((dep, style_gen(), help_state()), move |(dep, ..)| {
        view(dep).into()
    })
    .boxed()
}

/// Like `lazy`, but also rebuilds when the inline message-edit target changes.
/// Only use this for content that actually depends on `editing_event_id()`
/// (e.g. a timeline row, which swaps between plain text and an edit box) --
/// using it in place of `lazy` everywhere would invalidate every `lazy`'d
/// widget in the whole app on every edit start/stop, instead of just the
/// affected row.
pub fn lazy_editing<'a, Message, Dep, View>(
    dep: Dep,
    view: impl Fn(&Dep) -> View + 'a,
) -> Element<'a, Message>
where
    Message: 'static,
    Dep: Hash + 'a,
    View: Into<Element<'static, Message>> + 'static,
{
    w::lazy(
        (dep, style_gen(), help_state(), edit_gen()),
        move |(dep, ..)| view(dep).into(),
    )
    .boxed()
}

pub fn lazy_view<'a, Message, Dep, Action, OuterMessage>(
    dep: Dep,
    map_message: impl Fn(Message) -> OuterMessage + 'a,
) -> Element<'a, OuterMessage>
where
    Message: 'static,
    OuterMessage: 'static,
    Dep: Hash + IcedWidget<Message, Action> + 'a,
{
    w::lazy((dep, style_gen(), help_state()), |(dep, ..)| dep.view()).map_el(map_message)
}

pub trait DePlaceWidget<Message>: Widget<Message, iced::Theme, Renderer> {}

impl<Message, T> DePlaceWidget<Message> for T where T: Widget<Message, iced::Theme, Renderer> {}
