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
    sidebar: Arc<Sidebar>,
}

impl Hash for Home {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.sidebar.hash(state);
    }
}

impl Home {
    pub fn new(state: AppState) -> Self {
        Self {
            sidebar: Arc::new(Sidebar::new(state.clone())),
            state,
        }
    }

    pub fn subscription(&self) -> Subscription<HomeMessage> {
        self.sidebar.subscription().map(HomeMessage::Sidebar)
    }

    pub fn update(&mut self, message: HomeMessage) -> HomeAction {
        match message {
            HomeMessage::Sidebar(msg) => match Arc::make_mut(&mut self.sidebar).update(msg) {
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
