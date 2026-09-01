use header::{Header, HeaderMessage};
use macros::iced_cache;

use crate::common::*;
mod header;

#[derive(Debug, Clone)]
pub enum ChatMessage {
    Header(HeaderMessage),
}

pub enum ChatAction {
    Run(Task<()>),
}

#[iced_cache]
pub struct Chat {
    #[hash]
    header: Header,
}

impl Chat {
    pub fn new(state: &AppState, room: Room) -> Self {
        Self {
            header: Header::new(state, room),
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ChatMessage> {
        w::container(
            w::column![w::lazy(self.header.clone(), move |header| {
                header.view(theme, structure).map(ChatMessage::Header)
            })]
            .height(Fill),
        )
        .width(Fill)
        .height(Fill)
        .into()
    }
}

#[iced_cache]
pub struct EmptyChat {}

impl EmptyChat {
    pub fn new() -> Self {
        Self {}
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ChatMessage> {
        w::container("test").into()
    }
}
