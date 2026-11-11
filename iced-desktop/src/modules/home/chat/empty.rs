use macros::iced_cache;

use crate::common::*;

#[derive(Debug, Clone)]
pub enum EmptyChatMessage {}

#[derive(Debug, Clone)]
pub enum EmptyChatAction {}

#[iced_cache(Clone)]
pub struct EmptyChat {}

impl EmptyChat {
    pub fn new() -> Self {
        Self {}
    }
}

impl IcedWidget<EmptyChatMessage, EmptyChatAction> for EmptyChat {
    fn update(&mut self, _msg: EmptyChatMessage) -> Option<EmptyChatAction> {
        None
    }

    fn view(&self) -> Element<'static, EmptyChatMessage> {
        let theme = theme();
        let structure = structure();

        floating_tile(
            theme,
            structure,
            w::container(
                w::text("Empty chat is not implemented yet, click any room to view it")
                    .size(structure.font_size)
                    .color(theme.colors.warning),
            )
            .width(Fill)
            .height(Fill)
            .center(Fill),
        )
        .boxed()
    }
}
