use macros::iced_cache;

use crate::common::*;

#[derive(Debug, Clone)]
pub enum EmptyChatMessage {}

#[derive(Debug, Clone)]
pub enum EmptyChatAction {
    None,
}

#[iced_cache(Clone)]
pub struct EmptyChat {}

impl EmptyChat {
    pub fn new() -> Self {
        Self {}
    }
}

impl IcedWidget<EmptyChatMessage, EmptyChatAction> for EmptyChat {
    fn update(&mut self, _msg: EmptyChatMessage) -> EmptyChatAction {
        EmptyChatAction::None
    }

    fn view(&self, _theme: Theme, _structure: Structure) -> Element<'static, EmptyChatMessage> {
        w::container("test").into()
    }
}
