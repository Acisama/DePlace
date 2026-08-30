use crate::{
    common::*,
    components::sidebar::{Sidebar, SidebarMessage},
};

pub enum HomeMessage {
    Sidebar(SidebarMessage),
}

pub struct Home {
    state: AppState,
    sidebar: Sidebar,
}

impl Hash for Home {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.sidebar.hash(state);
    }
}

impl Home {
    pub fn new(state: AppState) -> Self {
        Self {
            sidebar: Sidebar::new(state.clone()),
            state,
        }
    }

    pub fn update(&mut self, message: HomeMessage) {
        match message {
            HomeMessage::Sidebar(sidebar) => match sidebar {
                SidebarMessage::ActiveRoomChange(room) => self.state.set_active_room(room),
                SidebarMessage::ActiveServerChange(server) => self.state.set_active_server(server),
            },
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, HomeMessage> {
        container(w::row![
            self.sidebar
                .view(theme, structure)
                .map(HomeMessage::Sidebar)
        ])
        .padding(structure.gap)
        .width(Fill)
        .height(Fill)
        .into()
    }
}
