use deplace_core::state::AppState;
use iced::{Element, advanced::text::Input, widget::text_input};
use macros::iced_cache;

use crate::{AppMessage, common::IcedWidget};
pub const QUICK_SELECT_INPUT_ID: &str = "quick_select_input";

#[derive(Clone)]
#[iced_cache]
pub struct QuickSelect {
    state: AppState,

    #[hash]
    input: String,
}

#[derive(Clone, Debug)]
pub enum QuickSelectMessage {
    Input(String),
}

pub enum QuickSelectAction {
    None,
}

impl IcedWidget<QuickSelectMessage, QuickSelectAction> for QuickSelect {
    fn update(&mut self, message: QuickSelectMessage) -> QuickSelectAction {
        tracing::debug!("{:?}", message);
        match message {
            QuickSelectMessage::Input(input) => self.input = input,
        }
        tracing::debug!("{:?}", self.input);
        QuickSelectAction::None
    }
    fn view(
        &self,
        theme: crate::common::Theme,
        structure: crate::common::Structure,
    ) -> iced::Element<'static, QuickSelectMessage> {
        text_input("Search for rooms...", self.input.clone())
            .id(QUICK_SELECT_INPUT_ID)
            .on_input(QuickSelectMessage::Input)
            .into()
    }
}

impl QuickSelect {
    pub fn new(state: &AppState) -> Self {
        Self {
            state: state.clone(),
            input: String::from(""),
        }
    }
}
