use std::process::exit;

use crate::common::*;
use chat::{
    Chat, ChatAction, ChatMessage, TimelineItemMessage, TimelineMessage,
    empty::{EmptyChat, EmptyChatMessage},
};
use deplace_core::{
    PaginationDirection,
    keybinds::{KeybindAction, Keybinds},
    state::ActiveServer,
};
use iced::{
    advanced::widget::{operate, operation::focusable::unfocus},
    widget::stack,
};
use lru::LruCache;
use macros::{iced_cache, nonzero_usize};
use sidebar::{Sidebar, SidebarAction, SidebarMessage};

pub mod overlay;

use overlay::{Overlay, OverlayAction, OverlayMessage};
use phosphor_svgs::icon as icons;

mod chat;
mod sidebar;

pub const COPY_ICON: &str = icons::copy::BOLD;
pub const EDIT_ICON: &str = icons::pencil_simple::BOLD;
pub const REPLY_ICON: &str = icons::arrow_bend_up_left::BOLD;
pub const PIN_ICON: &str = icons::push_pin::BOLD;
pub const UNPIN_ICON: &str = icons::push_pin_slash::BOLD;
pub const REDACT_ICON: &str = icons::trash::BOLD;
pub const DISCONNECT: &str = phosphor_svgs::icon::phone_disconnect::REGULAR;
pub const CONNECT: &str = phosphor_svgs::icon::phone::REGULAR;

pub const SPEAKER_ICON: &str = icons::speaker_high::FILL;
pub const SPACE_ICON: &str = icons::house_simple::BOLD;
pub const DEFAULT_ROOM_ICON: &str = icons::hash::BOLD;
pub const MIC_ICON: &str = icons::microphone::FILL;
pub const MUTE_ICON: &str = icons::microphone_slash::FILL;

pub const SCREENSHARING_ICON: &str = icons::monitor::BOLD;

pub const CHECKMARK_ICON: &str = icons::check::BOLD;

pub const SOURCE_ICON: &str = icons::code::BOLD;
pub const DOWNLOAD_ICON: &str = icons::download_simple::BOLD;

#[derive(Clone, Debug)]
pub enum HomeMessage {
    Chat {
        room_id: OwnedRoomId,
        message: ChatMessage,
    },
    EmptyChat(EmptyChatMessage),
    Sidebar(SidebarMessage),
    ActiveRoomChanged(Option<DePlaceRoom>),
    KeyboardEvent(iced::keyboard::Event),
    Overlay(OverlayMessage),
    TimelineScrollFinished {
        room_id: OwnedRoomId,
        direction: PaginationDirection,
        finished: bool,
    },
    SettingsChanged,
    /// The window regained focus -- the frontend may now mark the open
    /// room as read if its newest message was rendered earlier.
    WindowFocused,
    /// Notify of updated call state
    CallTick,
    SyncTick,
    MediaProgressTick,
    HelpToggle,
    HelpHover(Option<HelpKey>),
    HelpExit,
}

pub enum HomeAction {
    /// A Task that doesn't produce an output
    ///
    /// This is mostly used to run matrix sdk async functions,
    /// which don't produce an output but alter the client and
    /// it's internal state directly.
    Run(Task<()>),
    /// A Task that produces an output
    ///
    /// The ouput will be fed into the app as a message after
    /// completion.
    Perform(Task<HomeMessage>),
    LoadTimeline(Task<(OwnedRoomId, ChatMessage)>),
    TimelineScroll {
        room_id: OwnedRoomId,
        direction: PaginationDirection,
        task: Task<bool>,
    },
}

#[iced_cache(Clone)]
pub struct Home {
    state: AppState,
    keybinds: Receiver<Keybinds>,

    #[hash]
    sidebar: Sidebar,

    active_room_id: Option<OwnedRoomId>,

    chats: LruCache<OwnedRoomId, Chat>,
    /// Used when active Room is None
    #[hash]
    empty_chat: EmptyChat,

    window_title: Receiver<String>,

    #[hash]
    overlay: Overlay,

    #[hash]
    help: HelpState<HelpKey>,
}

impl ExtraHash for Home {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if let Some(chat) = self
            .active_room_id
            .as_ref()
            .and_then(move |id| self.chats.peek(id))
        {
            chat.hash(state);
        } else {
            self.empty_chat.hash(state);
        }
    }
}

impl Home {
    pub fn state(&self) -> &AppState {
        &self.state
    }
    pub fn new(state: AppState) -> (Self, Task<HomeMessage>) {
        let active_room = state.active_room();
        let initial_room = active_room.borrow().clone();

        let home = Self {
            sidebar: Sidebar::new(&state),
            overlay: Overlay::new(&state),
            keybinds: state.keybinds(),

            window_title: state.window_title(),

            active_room_id: initial_room.as_ref().map(|r| r.room_id().to_owned()),
            chats: LruCache::new(nonzero_usize!(100)),

            empty_chat: EmptyChat::new(),

            help: HelpState::default(),

            state: state.clone(),
        };

        let initial_load = Task::done(HomeMessage::ActiveRoomChanged(initial_room));

        let watch_task = Task::stream(iced::futures::stream::unfold(
            active_room,
            |mut rx| async move {
                rx.changed().await.ok()?;
                let room = rx.borrow().clone();
                Some((HomeMessage::ActiveRoomChanged(room), rx))
            },
        ));

        let settings_watch_task = Task::stream(iced::futures::stream::unfold(
            state.settings().watch_any_change(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((HomeMessage::SettingsChanged, rx))
            },
        ));

        let sync_tick_task = Task::stream(iced::futures::stream::unfold(
            state.sync_tick(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((HomeMessage::SyncTick, rx))
            },
        ));

        // The processor bumps the tick after every downloaded chunk, which can be
        // thousands of times per second. Coalesce them so that poking the
        // timeline rows on every tick stays cheap.
        const MEDIA_PROGRESS_TICK_INTERVAL: std::time::Duration =
            std::time::Duration::from_millis(100);

        let media_progress_tick_task = Task::stream(iced::futures::stream::unfold(
            (
                state.media_processor().progress_tick(),
                std::time::Instant::now(),
            ),
            |(mut rx, last_emit)| async move {
                rx.changed().await.ok()?;

                let now = std::time::Instant::now();
                let earliest = last_emit + MEDIA_PROGRESS_TICK_INTERVAL;
                if now < earliest {
                    tokio::time::sleep_until(earliest.into()).await;
                }

                Some((
                    HomeMessage::MediaProgressTick,
                    (rx, std::time::Instant::now()),
                ))
            },
        ));

        let member_update_task = Task::stream(iced::futures::stream::unfold(
            state.member_update_receiver(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((HomeMessage::SyncTick, rx))
            },
        ));

        let room_update_task = Task::stream(iced::futures::stream::unfold(
            state.room_update_receiver(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((HomeMessage::SyncTick, rx))
            },
        ));

        let calls_tick_task = Task::stream(iced::futures::stream::unfold(
            state.calls_tick_receiver(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((HomeMessage::CallTick, rx))
            },
        ));

        // Fires when a call is joined or left (per-call changes arrive
        // through each call's own state channel on top of this)
        let call_registry = state.call_registry();
        let call_watch_task = Task::stream(iced::futures::stream::unfold(
            call_registry.watch(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((HomeMessage::CallTick, rx))
            },
        ));

        (
            home,
            Task::batch([
                initial_load,
                watch_task,
                settings_watch_task,
                sync_tick_task,
                media_progress_tick_task,
                member_update_task,
                room_update_task,
                call_watch_task,
                calls_tick_task,
            ]),
        )
    }

    pub fn set_frontend_focused(&mut self, focused: bool) {
        self.state.set_window_focused(focused);
    }

    pub fn title(&self) -> String {
        self.window_title.borrow().clone()
    }

    fn load_room(&mut self, room: DePlaceRoom) -> Option<HomeAction> {
        let id = room.room_id().to_owned();
        let previous = self.active_room_id.replace(id.clone());

        // Keep chats aware of which one is displayed, so only that one
        // short-circuits read marking when new messages arrive.
        if previous != Some(id.clone())
            && let Some(previous) = &previous
            && let Some(chat) = self.chats.peek_mut(previous)
        {
            chat.set_active(false);
        }
        if let Some(chat) = self.chats.peek_mut(&id) {
            chat.set_active(true);
        }

        let restore_scroll = Task::done((
            id.clone(),
            ChatMessage::Timeline(TimelineMessage::RestoreScrollPosition),
        ));

        if self.chats.promote(&id) {
            return Some(HomeAction::LoadTimeline(Task::batch([
                restore_scroll,
                self.focus_input_task(&id),
            ])));
        }

        let (chat, task) = Chat::new(&self.state, room);

        // Use `push` instead of `put` in order to receive the old chat entry
        // and manually drop the `text_input::Content` behind the raw pointer
        // to avoid memory leaks
        // TODO: Also implement this for the whole chat
        if let Some((old_room_id, chat)) = self.chats.push(id.clone(), chat) {
            unsafe { std::mem::drop(Box::from_raw(chat.get_input_pointer())) };
            remove_editing_content(&old_room_id);
        };
        self.chats.promote(&id);
        if let Some(chat) = self.chats.peek_mut(&id) {
            chat.set_active(true);
        }

        Some(HomeAction::LoadTimeline(Task::batch([
            task,
            restore_scroll,
            self.focus_input_task(&id),
        ])))
    }

    fn focus_input_task(&self, id: &OwnedRoomId) -> Task<(OwnedRoomId, ChatMessage)> {
        let Some(chat) = self.chats.peek(id) else {
            return Task::none();
        };

        // If this room has a message mid-edit, `RestoreScrollPosition`'s handler
        // focuses that edit box instead, once the scroll position (and thus the
        // edit box's row existing in the virtualized list at all) is restored --
        // which happens strictly after this, overriding the focus set here.
        let id = id.clone();
        chat.focus_input::<()>()
            .map(move |_| (id.clone(), ChatMessage::None))
    }

    fn set_help_hovered(&mut self, key: Option<HelpKey>) -> Option<HomeAction> {
        self.help.hovered = key;
        None
    }

    fn set_active_server_task(&mut self, server: ActiveServer) -> Option<HomeAction> {
        let state = self.state.clone();
        Some(HomeAction::Run(Task::future(async move {
            state.set_active_server(server, true).await;
        })))
    }

    fn set_active_room_task(&mut self, room: Option<DePlaceRoom>) -> Option<HomeAction> {
        let state = self.state.clone();
        Some(HomeAction::Run(Task::future(async move {
            state
                .set_active_server(state.set_active_room(room).await, false)
                .await;
        })))
    }

    fn clear_active_chat_hover(&mut self) {
        if let Some(room_id) = &self.active_room_id
            && let Some(chat) = self.chats.peek_mut(room_id)
        {
            chat.clear_hover();
        }
    }

    fn dispatch_to_chat(&mut self, room_id: OwnedRoomId, msg: ChatMessage) -> Option<HomeAction> {
        let Some(chat) = self.chats.get_mut(&room_id) else {
            tracing::warn!(
                "Dropping message for room {}: no chat cached for it",
                room_id
            );
            return None;
        };

        if let Some(action) = chat.update(msg) {
            match action {
                ChatAction::ContextMenu(menu) => {
                    chat.clear_hover();
                    self.overlay.open_context_menu(*menu);
                    None
                }
                ChatAction::OpenHelpMenu => {
                    self.help.active = true;
                    None
                }
                ChatAction::HelpHover(help) => {
                    self.set_help_hovered(help);
                    None
                }
                ChatAction::Run(task) => Some(HomeAction::Run(task)),
                ChatAction::TimelineScroll { direction, task } => {
                    Some(HomeAction::TimelineScroll {
                        room_id: chat.room_id.clone(),
                        direction,
                        task,
                    })
                }
                ChatAction::Perform(task) => Some(HomeAction::Perform(task.map(move |message| {
                    HomeMessage::Chat {
                        room_id: room_id.clone(),
                        message,
                    }
                }))),
                ChatAction::JoinCall => join_call(&self.state, chat.room.clone()),
                ChatAction::LeaveCall => leave_call(&self.state, &chat.room),
                ChatAction::ShowProfile { member, bounds } => {
                    chat.clear_hover();
                    self.overlay.open_profile(member, bounds);
                    None
                }
                ChatAction::OpenModifyItem(modify) => {
                    chat.clear_hover();
                    self.overlay.open_modify_item(modify);
                    Some(HomeAction::Run(operate(unfocus())))
                }
                ChatAction::OpenMediaOverlay(params) => {
                    chat.clear_hover();
                    self.overlay.open_media_overlay(params);
                    None
                }
            }
        } else {
            None
        }
    }

    fn handle_overlay_message(&mut self, message: OverlayMessage) -> Option<HomeAction> {
        match self.overlay.update(message)? {
            OverlayAction::HelpHover(help_key) => {
                self.set_help_hovered(help_key);
                None
            }
            OverlayAction::Perform(task) => {
                Some(HomeAction::Perform(task.map(HomeMessage::Overlay)))
            }
            OverlayAction::Run(task) => Some(HomeAction::Run(task)),
            OverlayAction::ChangeRoom(room) => self.set_active_room_task(room),
            OverlayAction::SetIsReplyingTo {
                item_id,
                room_id,
                event_id,
            } => self.dispatch_to_chat(
                room_id,
                ChatMessage::Timeline(TimelineMessage::Item {
                    id: item_id,
                    message: TimelineItemMessage::SetIsReplyingTo(event_id),
                }),
            ),
            OverlayAction::SetIsEditing { item_id, room_id } => self.dispatch_to_chat(
                room_id,
                ChatMessage::Timeline(TimelineMessage::Item {
                    id: item_id,
                    message: TimelineItemMessage::SetIsEditing(true),
                }),
            ),
        }
    }
}

/// Joins the MatrixRTC call in a room: gets a LiveKit token from the room's
/// RTC focus, signals the join to the room and plays all call audio there
fn join_call(state: &AppState, room: DePlaceRoom) -> Option<HomeAction> {
    let Some(audio) = state.audio() else {
        tracing::warn!(
            "Not joining the call in {}: no audio connection",
            room.room_id()
        );
        return None;
    };

    let device = state.own_device();
    let manager = state.livekit_room_manager();
    let call_registry = state.call_registry();
    let calls_tick = state.calls_tick().clone();
    let profile_store = state.profile_store();

    // Spamming the button shouldn't try to join the call multiple times
    if room.call().join_progress().is_some() || call_registry.get(room.room_id()).is_some() {
        return None;
    }

    Some(HomeAction::Run(Task::future(async move {
        // The returned handle is already registered in the call registry,
        // from where the frontend can reach it via `call_registry.get`.
        if let Err(e) = deplace_core::matrix_api::matrixrtc::join_matrixrtc_call(
            room,
            profile_store,
            device,
            manager,
            call_registry,
            audio,
            calls_tick,
        )
        .await
        {
            tracing::error!("Could not join the call: {e:#}");
        }
    })))
}

/// Leaves the MatrixRTC call in a room, if there is one
fn leave_call(state: &AppState, room: &DePlaceRoom) -> Option<HomeAction> {
    let call = state.call_registry().get(room.room_id());

    if call.is_none() {
        // No live call is known, but the room can still list us as in the
        // call,  a leftover from a crashed session or a join that failed
        // halfway. Withdrawing the membership still lets the user leave.
        tracing::warn!(
            "No live call known in {}, withdrawing the call membership anyway",
            room.room_id()
        );
    }

    let livekit_room_manager = state.livekit_room_manager();
    let own_device = state.own_device();
    let room = room.room.clone();

    Some(HomeAction::Run(Task::future(async move {
        if let Some(call) = call {
            call.leave().await;
        } else if let Err(e) = deplace_core::matrix_api::matrixrtc::leave_matrixrtc_call(
            &room,
            &own_device,
            livekit_room_manager,
        )
        .await
        {
            tracing::error!(
                "Could not withdraw the call membership in {}: {e:#}",
                room.room_id()
            );
        }
    })))
}

impl IcedWidget<HomeMessage, HomeAction> for Home {
    fn update(&mut self, message: HomeMessage) -> Option<HomeAction> {
        match message {
            HomeMessage::Sidebar(msg) => match self.sidebar.update(msg)? {
                SidebarAction::JoinCall(room) => join_call(&self.state, room),
                SidebarAction::ParticipantMenu(menu) => {
                    self.overlay.open_context_menu(menu);
                    None
                }
                SidebarAction::ChangeRoom(room) => self.set_active_room_task(room),
                SidebarAction::ChangeServer(server) => self.set_active_server_task(server),
                SidebarAction::Run(task) => Some(HomeAction::Run(task)),
                SidebarAction::HelpHover(key) => self.set_help_hovered(key),
                SidebarAction::OpenSettings => {
                    self.overlay.toggle_settings();
                    if self.overlay.is_open() {
                        self.clear_active_chat_hover();
                    }
                    None
                }
                SidebarAction::OpenQuickselect => {
                    self.overlay.toggle_quick_select();
                    if self.overlay.is_open() {
                        self.clear_active_chat_hover();
                    }
                    None
                }
                SidebarAction::ShowProfile { member, bounds } => {
                    self.overlay.open_profile(member, bounds);
                    None
                }
            },
            HomeMessage::ActiveRoomChanged(Some(room)) => self.load_room(room),
            HomeMessage::ActiveRoomChanged(None) => {
                // Nothing is displayed anymore, so no chat may short-circuit
                // read marking until a room is opened again.
                if let Some(room_id) = &self.active_room_id
                    && let Some(chat) = self.chats.peek_mut(room_id)
                {
                    chat.set_active(false);
                }
                None
            }
            HomeMessage::Chat { room_id, message } => self.dispatch_to_chat(room_id, message),
            // TODO: Implement empty chat
            HomeMessage::EmptyChat(_) => {
                tracing::warn!("Empty chat not yet implemented");
                None
            }
            HomeMessage::KeyboardEvent(event) => {
                let keybinds = self.keybinds.borrow();

                if let Some(action) = keybinds.action_for_event(&event) {
                    drop(keybinds);
                    match action {
                        KeybindAction::ToggleQuickselect => {
                            let task = self.overlay.toggle_quick_select();
                            if self.overlay.is_open() {
                                self.clear_active_chat_hover();
                            }
                            return task.map(HomeAction::Run);
                        }
                        KeybindAction::ToggleSettings => {
                            let task = self.overlay.toggle_settings();
                            if self.overlay.is_open() {
                                self.clear_active_chat_hover();
                            }
                            return task.map(HomeAction::Run);
                        }
                        KeybindAction::ToggleOverview => {
                            if let Some(room_id) = &self.active_room_id {
                                return self.dispatch_to_chat(
                                    room_id.clone(),
                                    ChatMessage::ToggleOverview,
                                );
                            }
                        }
                        KeybindAction::ToggleHelp => {
                            self.help.active = !self.help.active;
                            return None;
                        }
                        KeybindAction::CloseApp => {
                            exit(1);
                            // iced::exit() doesn't close the window
                            // return Some(HomeAction::Run(iced::exit()));
                        }
                    };
                } else {
                    drop(keybinds);
                }

                if self.overlay.is_open() {
                    return self.handle_overlay_message(OverlayMessage::KeyboardEvent(event));
                }

                let id = self.active_room_id.clone()?;
                self.dispatch_to_chat(id, ChatMessage::KeyboardEvent(event))
            }
            HomeMessage::Overlay(message) => self.handle_overlay_message(message),
            HomeMessage::TimelineScrollFinished {
                room_id,
                direction,
                finished,
            } => {
                let chat = self.chats.peek_mut(&room_id)?;

                chat.set_timeline_scroll_finished(direction, finished);
                None
            }
            HomeMessage::SettingsChanged => {
                let chat = self
                    .active_room_id
                    .as_ref()
                    .and_then(|id| self.chats.peek_mut(id))?;

                chat.touch_all();
                None
            }
            HomeMessage::WindowFocused => {
                let room_id = self.active_room_id.clone()?;
                self.dispatch_to_chat(
                    room_id,
                    ChatMessage::Timeline(TimelineMessage::WindowFocused),
                )
            }
            HomeMessage::SyncTick => None,
            HomeMessage::CallTick => None,
            HomeMessage::MediaProgressTick => {
                // The virtualized list only re-examines a row's (lazy) hash when
                // poked, and video message hashes embed the download progress,
                // so poke the rows to refresh the progress display while media
                // is downloading.
                if let Some(room_id) = &self.active_room_id
                    && let Some(chat) = self.chats.peek_mut(room_id)
                {
                    chat.touch_all();
                }
                None
            }
            HomeMessage::HelpToggle => {
                self.help.active = true;
                None
            }
            HomeMessage::HelpHover(key) => self.set_help_hovered(key),
            HomeMessage::HelpExit => {
                self.help.active = false;
                self.help.hovered = None;
                None
            }
        }
    }

    fn view(&self) -> Element<'static, HomeMessage> {
        set_help_state(self.help);

        let sidebar = lazy_view(self.sidebar.clone(), HomeMessage::Sidebar);

        let structure = structure();

        let chat = match self
            .active_room_id
            .as_ref()
            .and_then(|id| self.chats.peek(id))
        {
            Some(chat) => {
                let room_id = chat.room_id.clone();
                w::container(lazy_view(chat.clone(), move |msg| HomeMessage::Chat {
                    room_id: room_id.clone(),
                    message: msg,
                }))
            }
            None => w::container(lazy_view(self.empty_chat.clone(), HomeMessage::EmptyChat)),
        }
        .width(Fill)
        .height(Fill);

        let main_content = w::container(w::row![sidebar, chat].height(Fill).spacing(structure.gap))
            .padding(structure.gap)
            .width(Fill)
            .height(Fill);

        let mut stack = stack![main_content];

        if self.overlay.is_open() {
            stack = stack.push(lazy_view(self.overlay.clone(), HomeMessage::Overlay));
        }

        help_root(
            &self.help,
            stack,
            HomeMessage::HelpHover,
            HomeMessage::HelpExit,
        )
        .into()
    }
}
