use crate::common::*;
use sidebar::{Sidebar, SidebarAction, SidebarMessage};

mod sidebar;

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

    window_title: Receiver<String>,
}

impl Hash for Home {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.sidebar.hash(state);
    }
}

impl Home {
    pub fn new(state: AppState) -> Self {
        Self {
            sidebar: Sidebar::new(&state),
            window_title: state.window_title(),

            state: state.clone(),
        }
    }

    pub fn title(&self) -> String {
        self.window_title.borrow().clone()
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
        w::container(
            w::row![w::lazy(self.sidebar.clone(), move |sidebar| {
                sidebar.view(theme, structure).map(HomeMessage::Sidebar)
            })]
            .height(Fill),
        )
        .padding(structure.gap)
        .width(Fill)
        .height(Fill)
        .into()
    }
}
