use crate::common::*;
use chat::{
    Chat, ChatAction, ChatMessage, TimelineMessage,
    empty::{EmptyChat, EmptyChatMessage},
};
use lru::LruCache;
use macros::{iced_cache, nonzero_usize};
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
}

pub enum HomeAction {
    Run(Task<()>),
    None,
    LoadTimeline(Task<(OwnedRoomId, TimelineMessage)>),
}

#[iced_cache]
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

    fn load_room(&mut self, room: Room) -> Option<Task<(OwnedRoomId, TimelineMessage)>> {
        let id = room.room_id().to_owned();
        self.active_room_id = Some(id.clone());

        if self.chats.promote(&id) {
            return None;
        }

        let (chat, task) = Chat::new(&self.state, room);

        // Use `push` instead of `put` in order to receive the old chat entry
        // and manually drop the `text_input::Content` behind the raw pointer
        // to avoid memory leaks
        if let Some((_, chat)) = self.chats.push(id.clone(), chat) {
            unsafe { std::mem::drop(Box::from_raw(chat.get_input_pointer())) };
        };
        self.chats.promote(&id);

        Some(task)
    }

    fn dispatch_to_chat(&mut self, room_id: &OwnedRoomId, msg: ChatMessage) -> HomeAction {
        let Some(chat) = self.chats.get_mut(room_id) else {
            tracing::warn!(
                "Dropping message for room {}: no chat cached for it",
                room_id
            );
            return HomeAction::None;
        };

        match chat.update(msg) {
            ChatAction::Run(task) => HomeAction::Run(task),
            ChatAction::None => HomeAction::None,
        }
    }
}

impl IcedWidget<HomeMessage, HomeAction> for Home {
    fn update(&mut self, message: HomeMessage) -> HomeAction {
        match message {
            HomeMessage::Sidebar(msg) => match self.sidebar.update(msg) {
                SidebarAction::Run(task) => return HomeAction::Run(task),
                SidebarAction::None => {}
            },
            HomeMessage::ActiveRoomChanged(Some(room)) => {
                if let Some(task) = self.load_room(room) {
                    return HomeAction::LoadTimeline(task);
                }
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
        }

        HomeAction::None
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

        w::container(w::row![sidebar, chat].height(Fill).spacing(structure.gap))
            .padding(structure.gap)
            .width(Fill)
            .height(Fill)
            .into()
    }
}
