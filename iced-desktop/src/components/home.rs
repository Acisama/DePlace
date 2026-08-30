use crate::{
    common::*,
    components::sidebar::{Sidebar, SidebarAction, SidebarMessage},
};

#[derive(Clone, Debug)]
pub enum HomeMessage {
    Sidebar(SidebarMessage),
}

pub enum HomeAction {
    EmptyRun(Task<()>),
    None,
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

    pub fn update(&mut self, message: HomeMessage) -> HomeAction {
        match message {
            HomeMessage::Sidebar(msg) => match self.sidebar.update(msg) {
                SidebarAction::Run(task) => return HomeAction::EmptyRun(task),
                SidebarAction::None => {}
            },
        }

        HomeAction::None
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
