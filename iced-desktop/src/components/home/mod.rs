use crate::{
    common::*,
    components::overlay::{Overlay, OverlayMessage, QUICK_SELECT_INPUT_ID},
    things::on_message,
};
use chat::{
    Chat, ChatAction, ChatMessage, TimelineMessage,
    empty::{EmptyChat, EmptyChatMessage},
};
use iced::{
    keyboard::{Key, Modifiers},
    widget::{operation::focus, stack},
};
use lru::LruCache;
use macros::{iced_cache, nonzero_usize};
use matrix_sdk::media::UniqueKey;
use sidebar::{Sidebar, SidebarAction, SidebarMessage};

mod chat;
mod sidebar;

#[derive(Clone, Debug)]
pub enum HomeMessage {
    Chat(ChatMessage),
    Timeline {
        room_id: OwnedRoomId,
        message: TimelineMessage,
    },
    EmptyChat(EmptyChatMessage),
    Sidebar(SidebarMessage),
    ActiveRoomChanged(Option<Room>),
    KeyboardEvent(iced::keyboard::Event),
    Overlay(OverlayMessage),
    MediaLoaded(MediaLoaded),
}

pub enum HomeAction {
    Run(Task<()>),
    LoadMediaTask(Task<MediaLoaded>),
    LoadTimeline(Task<(OwnedRoomId, TimelineMessage)>),
}

#[iced_cache(Clone)]
pub struct Home {
    state: AppState,

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

        state.client().add_event_handler(on_message);

        let home = Self {
            sidebar: Sidebar::new(&state),
            overlay: Overlay::None,

            window_title: state.window_title(),

            active_room_id: initial_room.as_ref().map(|r| r.room_id().to_owned()),
            chats: LruCache::new(nonzero_usize!(100)),

            empty_chat: EmptyChat::new(),

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

        (home, Task::batch([initial_load, watch_task]))
    }

    pub fn title(&self) -> String {
        self.window_title.borrow().clone()
    }

    fn load_room(&mut self, room: Room) -> Task<(OwnedRoomId, TimelineMessage)> {
        let id = room.room_id().to_owned();
        self.active_room_id = Some(id.clone());

        if self.chats.promote(&id) {
            return self.focus_input_task(&id);
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

        Task::batch([task, self.focus_input_task(&id)])
    }

    fn focus_input_task(&self, id: &OwnedRoomId) -> Task<(OwnedRoomId, TimelineMessage)> {
        let Some(chat) = self.chats.peek(id) else {
            return Task::none();
        };

        let id = id.clone();
        chat.focus_input()
            .map(move |_| (id.clone(), TimelineMessage::None))
    }

    fn load_media_task(&self, needs_media: NeedsMedia) -> Option<HomeAction> {
        let state = self.state.clone();
        Some(HomeAction::LoadMediaTask(Task::future(async move {
            match needs_media {
                NeedsMedia::Avatar { uri } => {
                    let (media, success) = state.avatar_cache().load_avatar(uri.clone()).await;
                    tracing::trace!(
                        "Loading of avatar {uri} finished: {}",
                        if success { "success" } else { "failure" }
                    );
                    media
                }
                NeedsMedia::Thumbnail { source, key } => {
                    let (media, success) = state
                        .thumbnail_cache()
                        .load_thumbnail(source.clone(), key)
                        .await;
                    tracing::trace!(
                        "Loading of thumbnail {} finished: {}",
                        source.unique_key(),
                        if success { "success" } else { "failure" }
                    );
                    media
                }
                NeedsMedia::Video { source, filename } => {
                    let (media, success) = state
                        .video_cache()
                        .load_video(source, filename.clone())
                        .await;
                    tracing::trace!(
                        "Loading of video {filename} finished: {}",
                        if success { "success" } else { "failure" }
                    );
                    media
                }
            }
        })))
    }

    fn dispatch_to_chat(&mut self, room_id: &OwnedRoomId, msg: ChatMessage) -> Option<HomeAction> {
        let Some(chat) = self.chats.get_mut(room_id) else {
            tracing::warn!(
                "Dropping message for room {}: no chat cached for it",
                room_id
            );
            return None;
        };

        if let Some(action) = chat.update(msg) {
            match action {
                ChatAction::Run(task) => Some(HomeAction::Run(task)),
                ChatAction::NeedsMedia(needs_media) => self.load_media_task(needs_media),
            }
        } else {
            None
        }
    }
}

impl IcedWidget<HomeMessage, HomeAction> for Home {
    fn update(&mut self, message: HomeMessage) -> Option<HomeAction> {
        match message {
            HomeMessage::Sidebar(msg) => {
                return if let Some(action) = self.sidebar.update(msg) {
                    match action {
                        SidebarAction::Run(task) => Some(HomeAction::Run(task)),
                        SidebarAction::NeedsMedia(needs_media) => self.load_media_task(needs_media),
                    }
                } else {
                    None
                };
            }
            HomeMessage::ActiveRoomChanged(Some(room)) => {
                return Some(HomeAction::LoadTimeline(self.load_room(room)));
            }
            HomeMessage::ActiveRoomChanged(None) => {}
            HomeMessage::Chat(msg) => {
                if let Some(id) = self.active_room_id.clone() {
                    return self.dispatch_to_chat(&id, msg);
                }
            }
            HomeMessage::Timeline { room_id, message } => {
                return self.dispatch_to_chat(&room_id, ChatMessage::Timeline(message));
            }
            // TODO: Implement empty chat
            HomeMessage::EmptyChat(_) => tracing::warn!("Empty chat not yet implemented"),
            HomeMessage::KeyboardEvent(event) => {
                if !matches!(self.overlay, Overlay::None) {
                    self.overlay.update(OverlayMessage::KeyboardEvent(event));
                    return None;
                }
                let iced::keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    repeat,
                    ..
                } = event
                else {
                    return None;
                };
                if !repeat
                    && modifiers == Modifiers::CTRL
                    && Key::Character("k".into()) == key
                    && !matches!(self.overlay, Overlay::QuickSelect(_))
                {
                    self.overlay.open_quick_select(&self.state);
                    return Some(HomeAction::Run(focus(QUICK_SELECT_INPUT_ID)));
                }
            }
            HomeMessage::Overlay(msg) => {
                self.overlay.update(msg);
            }
            HomeMessage::MediaLoaded(media) => {
                self.sidebar.load_media(&media);

                if let Some(chat) = self
                    .active_room_id
                    .as_ref()
                    .and_then(|id| self.chats.peek_mut(id))
                {
                    chat.load_media(&media);
                }
            }
        }

        None
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, HomeMessage> {
        let sidebar = w::lazy(self.sidebar.clone(), move |sidebar| {
            sidebar.view(theme, structure).map(HomeMessage::Sidebar)
        });

        let chat = match self
            .active_room_id
            .as_ref()
            .and_then(|id| self.chats.peek(id))
        {
            Some(chat) => w::container(w::lazy(chat.clone(), move |chat| {
                chat.view(theme, structure).map(HomeMessage::Chat)
            })),
            None => w::container(w::lazy(self.empty_chat.clone(), move |chat| {
                chat.view(theme, structure).map(HomeMessage::EmptyChat)
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

        if !matches!(self.overlay, Overlay::None) {
            let overlay = w::lazy(self.overlay.clone(), move |overlay| {
                overlay.view(theme, structure).map(HomeMessage::Overlay)
            });
            stack = stack.push(overlay);
        }
        stack.into()
    }
}
