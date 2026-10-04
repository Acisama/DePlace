use crate::common::*;
use chat::{
    Chat, ChatAction, ChatMessage, TimelineItemMessage, TimelineMessage,
    empty::{EmptyChat, EmptyChatMessage},
};
use deplace_core::{
    PaginationDirection,
    keybinds::{KeybindAction, Keybinds},
    state::{ActiveServer, cache::CacheResultStatus},
};
use iced::widget::stack;
use lru::LruCache;
use macros::{iced_cache, nonzero_usize};
use matrix_sdk::media::UniqueKey;
use sidebar::{Sidebar, SidebarAction, SidebarMessage};

pub mod overlay;

use overlay::{Overlay, OverlayAction, OverlayMessage};

mod chat;
mod sidebar;

/// Identifies a help-mode target. Give each distinct explorable element its
/// own variant; see [`crate::components::help_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpKey {
    Sidebar(SidebarHelpKey),
    Chat(ChatHelpKey),
    ChatSidebar(ChatSidebarHelpKey),
    Header(HeaderHelpKey),
    Message { key: MessageHelpKey, index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeaderHelpKey {
    Header,
    HeaderIcon,
    HeaderName,
    CallButton,
    OpenPins,
    OpenMemberList,
    SearchInput,
    HelpButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatHelpKey {
    Chat,
    MainPanel,
    Input,
    UploadButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarHelpKey {
    Sidebar,
    HomeIcon,
    ServerColumn,
    DmsWithNotifications,
    Servers,
    Server(usize),

    QuickselectButton,

    Channels,
    Dms,
    ServerName,
    Channel(usize),

    Account,
    SettingsButton,
    AccountIcon,
    AccountName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatSidebarHelpKey {
    ChatSidebar,
    PinnedMessage,
    SearchResultMessage,
    Banner,
    BannerUserIcon,
    MemberName,
    MemberId,
    OnlineMembers,
    OfflineMembers,
    OnlineMemberCount,
    OfflineMemberCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageHelpKey {
    Message,
    Date,
    Name,
    Icon,
    Reply(usize),
    TextContent,
    OtherContent,
}

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
    MediaLoaded(MediaLoaded),
    TimelineScrollFinished {
        room_id: OwnedRoomId,
        direction: PaginationDirection,
        finished: bool,
    },
    SettingsChanged,
    SyncTick,
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
    LoadMediaTask(Task<MediaLoaded>),
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

        (
            home,
            Task::batch([
                initial_load,
                watch_task,
                settings_watch_task,
                sync_tick_task,
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
        self.active_room_id = Some(id.clone());

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
        if let Some((_, chat)) = self.chats.push(id.clone(), chat) {
            unsafe { std::mem::drop(Box::from_raw(chat.get_input_pointer())) };
        };
        self.chats.promote(&id);

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

        let id = id.clone();
        chat.focus_input()
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

    fn load_media_task(&self, needs_media: NeedsMedia) -> Option<HomeAction> {
        let state = self.state.clone();
        Some(HomeAction::LoadMediaTask(Task::future(async move {
            match needs_media {
                NeedsMedia::Avatar { uri } => {
                    let res = state.avatar_cache().load_content(&uri).await;
                    match res.status {
                        CacheResultStatus::CacheHit => {}
                        CacheResultStatus::Success => {
                            tracing::trace!("Loading of avatar {uri} finished successfully");
                        }
                        CacheResultStatus::Failure(e) => {
                            tracing::error!("Loading of avatar {uri} failed: {e}");
                        }
                    }
                    res.loaded
                }
                NeedsMedia::Thumbnail { source, key } => {
                    let res = state
                        .thumbnail_cache()
                        .load_content_with_key(&source, key)
                        .await;
                    match res.status {
                        CacheResultStatus::CacheHit => {}
                        CacheResultStatus::Success => {
                            tracing::trace!(
                                "Loading of thumbnail {} finished successfully",
                                source.unique_key()
                            );
                        }
                        CacheResultStatus::Failure(e) => {
                            tracing::error!(
                                "Loading of thumbnail {} failed: {e}",
                                source.unique_key()
                            );
                        }
                    }
                    res.loaded
                }
                NeedsMedia::Video { source } => {
                    let unique_key = source.unique_key();
                    let res = state.video_cache().load_content(&source).await;
                    match res.status {
                        CacheResultStatus::CacheHit => {}
                        CacheResultStatus::Success => {
                            tracing::trace!(
                                "Loading of video {} finished successfully",
                                unique_key
                            );
                        }
                        CacheResultStatus::Failure(e) => {
                            tracing::error!("Loading of video {} failed: {e}", unique_key);
                        }
                    }
                    res.loaded
                }
            }
        })))
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
                    self.overlay.open_context_menu(menu);
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
                ChatAction::NeedsMedia(needs_media) => self.load_media_task(needs_media),
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
                ChatAction::JoinCall => join_call(&chat.room_id),
                ChatAction::LeaveCall => leave_call(&chat.room_id),
                ChatAction::ShowProfile {
                    room_id,
                    user_id,
                    bounds,
                } => {
                    self.overlay.open_profile(room_id, user_id, bounds);
                    None
                }
                ChatAction::OpenModifyItem(modify) => {
                    self.overlay.open_modify_item(modify);
                    None
                }
            }
        } else {
            None
        }
    }

    fn handle_overlay_message(&mut self, message: OverlayMessage) -> Option<HomeAction> {
        match self.overlay.update(message)? {
            OverlayAction::Perform(task) => {
                Some(HomeAction::Perform(task.map(HomeMessage::Overlay)))
            }
            OverlayAction::Run(task) => Some(HomeAction::Run(task)),
            OverlayAction::NeedsMedia(media) => self.load_media_task(media),
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
        }
    }
}

fn join_call(room_id: &RoomId) -> Option<HomeAction> {
    tracing::trace!("Join room {}", room_id);
    None
}

fn leave_call(room_id: &RoomId) -> Option<HomeAction> {
    tracing::trace!("Leave room {}", room_id);
    None
}

impl IcedWidget<HomeMessage, HomeAction> for Home {
    fn update(&mut self, message: HomeMessage) -> Option<HomeAction> {
        match message {
            HomeMessage::Sidebar(msg) => match self.sidebar.update(msg)? {
                SidebarAction::NeedsMedia(needs_media) => self.load_media_task(needs_media),
                SidebarAction::ChangeRoom(room) => self.set_active_room_task(room),
                SidebarAction::ChangeServer(server) => self.set_active_server_task(server),
                SidebarAction::Run(task) => Some(HomeAction::Run(task)),
                SidebarAction::HelpHover(key) => self.set_help_hovered(key),
                SidebarAction::OpenSettings => {
                    self.overlay.toggle_settings();
                    None
                }
                SidebarAction::OpenQuickselect => {
                    self.overlay.toggle_quick_select();
                    None
                }
            },
            HomeMessage::ActiveRoomChanged(Some(room)) => self.load_room(room),
            HomeMessage::ActiveRoomChanged(None) => None,
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
                            return self.overlay.toggle_quick_select().map(HomeAction::Run);
                        }
                        KeybindAction::ToggleSettings => {
                            return self.overlay.toggle_settings().map(HomeAction::Run);
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
            HomeMessage::MediaLoaded(media) => {
                let chat = self
                    .active_room_id
                    .as_ref()
                    .and_then(|id| self.chats.peek_mut(id))?;

                chat.touch_media(&media);
                None
            }
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
            HomeMessage::SyncTick => None,
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

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        _: HelpState<HelpKey>,
    ) -> Element<'static, HomeMessage> {
        let help_state = self.help;
        let sidebar = w::lazy(
            (self.sidebar.clone(), help_state),
            move |(sidebar, help_state)| {
                sidebar
                    .view(theme, structure, *help_state)
                    .map(HomeMessage::Sidebar)
            },
        );

        let chat = match self
            .active_room_id
            .as_ref()
            .and_then(|id| self.chats.peek(id))
        {
            Some(chat) => w::container(w::lazy(
                (chat.clone(), help_state),
                move |(chat, help_state)| {
                    let room_id = chat.room_id.clone();
                    chat.view(theme, structure, *help_state)
                        .map(move |msg| HomeMessage::Chat {
                            room_id: room_id.clone(),
                            message: msg,
                        })
                },
            )),
            None => w::container(w::lazy(self.empty_chat.clone(), move |chat| {
                chat.view(theme, structure, help_state)
                    .map(HomeMessage::EmptyChat)
            }))
            .padding(structure.gap),
        }
        .width(Fill)
        .height(Fill);

        let main_content = w::container(w::row![sidebar, chat].height(Fill).spacing(structure.gap))
            .padding(structure.gap)
            .width(Fill)
            .height(Fill);

        let mut stack = stack![main_content];

        if self.overlay.is_open() {
            let overlay = w::lazy(self.overlay.clone(), move |overlay| {
                overlay
                    .view(theme, structure, help_state)
                    .map(HomeMessage::Overlay)
            });
            stack = stack.push(overlay);
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
