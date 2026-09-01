use std::collections::HashMap;

use crate::common::*;
use chat::{Chat, ChatAction, ChatMessage, EmptyChat};
use macros::iced_cache;
use sidebar::{Sidebar, SidebarAction, SidebarMessage};

mod chat;
mod sidebar;

#[derive(Clone, Debug)]
pub enum HomeMessage {
    Chat(ChatMessage),
    Sidebar(SidebarMessage),
    ActiveRoomChanged(Option<Room>),
}

pub enum HomeAction {
    EmptyRun(Task<()>),
    None,
}

#[iced_cache]
pub struct Home {
    state: AppState,

    #[hash]
    sidebar: Sidebar,

    active_room_id: Option<OwnedRoomId>,

    chats: HashMap<OwnedRoomId, Chat>,
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
            .and_then(move |id| self.chats.get(id))
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

        let mut home = Self {
            sidebar: Sidebar::new(&state),
            window_title: state.window_title(),

            active_room_id: active_room
                .borrow()
                .as_ref()
                .map(|r| r.room_id().to_owned()),
            chats: HashMap::default(),

            empty_chat: EmptyChat::new(),

            state: state.clone(),
        };

        if let Some(room) = active_room.borrow().as_ref() {
            home.load_room(room.clone());
        }

        let task = Task::stream(iced::futures::stream::unfold(
            active_room,
            |mut rx| async move {
                rx.changed().await.ok()?;
                let room = rx.borrow().clone();
                Some((HomeMessage::ActiveRoomChanged(room), rx))
            },
        ));

        (home, task)
    }

    pub fn title(&self) -> String {
        self.window_title.borrow().clone()
    }

    fn load_room(&mut self, room: Room) {
        let id = room.room_id().to_owned();
        self.active_room_id = Some(id.clone());

        self.chats
            .entry(id)
            .or_insert_with(|| Chat::new(&self.state, room));
    }
}

impl IcedWidget<HomeMessage, HomeAction> for Home {
    fn update(&mut self, message: HomeMessage) -> HomeAction {
        match message {
            HomeMessage::Sidebar(msg) => match self.sidebar.update(msg) {
                SidebarAction::Run(task) => return HomeAction::EmptyRun(task),
                SidebarAction::None => {}
            },
            HomeMessage::ActiveRoomChanged(Some(room)) => self.load_room(room),
            HomeMessage::ActiveRoomChanged(None) => {}
            HomeMessage::Chat(msg) => {
                if let Some(id) = &self.active_room_id
                    && let Some(chat) = self.chats.get_mut(id)
                {
                    match chat.update(msg) {
                        ChatAction::Run(task) => return HomeAction::EmptyRun(task),
                        ChatAction::None => {}
                    }
                }
            }
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
            .and_then(|id| self.chats.get(id))
        {
            Some(chat) => w::container(w::lazy(chat.clone(), move |chat| {
                chat.view(theme, structure).map(HomeMessage::Chat)
            })),
            None => w::container(w::lazy(self.empty_chat.clone(), move |chat| {
                chat.view(theme, structure).map(HomeMessage::Chat)
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
