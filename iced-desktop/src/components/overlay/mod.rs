use deplace_core::state::AppState;
use iced::{
    Color,
    Length::Fill,
    keyboard::{Key, Modifiers},
    widget::{container, mouse_area, opaque},
};

use crate::{
    common::IcedWidget,
    components::overlay::quick_select::{QuickSelect, QuickSelectMessage},
};

pub use quick_select::QUICK_SELECT_INPUT_ID;

mod quick_select;

#[derive(Clone, Default, Hash)]
pub enum Overlay {
    #[default]
    None,
    QuickSelect(QuickSelect),
    Settings,
}

#[derive(Clone, Debug)]
pub enum OverlayMessage {
    Close,
    QuickSelect(QuickSelectMessage),
    KeyboardEvent(iced::keyboard::Event),
}

impl Overlay {
    /// Open the quick select overlay
    ///
    /// This function takes care of closing any other overlay
    /// and opening quick select instead
    pub fn open_quick_select(&mut self, state: &AppState) {
        let quick_select = QuickSelect::new(state);
        *self = Self::QuickSelect(quick_select)
    }

    /// Close the currently open overlay
    ///
    /// This function takes care of the logic needed to close the
    /// currently open overlay
    pub fn close_overlay(&mut self) {
        *self = Self::None
    }
}

pub enum OverlayAction {}

impl IcedWidget<OverlayMessage, OverlayAction> for Overlay {
    fn update(&mut self, message: OverlayMessage) -> Option<OverlayAction> {
        match message {
            OverlayMessage::Close => {
                self.close_overlay();
            }
            OverlayMessage::QuickSelect(msg) => {
                let Overlay::QuickSelect(qs) = self else {
                    tracing::warn!("Received quick select message without it being open");
                    return None;
                };
                qs.update(msg);
            }
            OverlayMessage::KeyboardEvent(event) => {
                if let iced::keyboard::Event::KeyPressed {
                    key: Key::Character(ref k),
                    modifiers: Modifiers::CTRL,
                    repeat: false,
                    ..
                } = event
                {
                    // here I will handle the keypress in the overlay, but I should think
                    // about bubbling the event to the component itself
                    if k == "k" && matches!(self, Overlay::QuickSelect(_)) {
                        self.close_overlay();
                    }
                }
                if let iced::keyboard::Event::KeyPressed { key, .. } = event {
                    if matches!(key, Key::Named(iced::keyboard::key::Named::Escape)) {
                        self.close_overlay();
                    }
                }
            }
        }
        None
    }

    fn view(
        &self,
        theme: crate::common::Theme,
        structure: crate::common::Structure,
    ) -> iced::Element<'static, OverlayMessage> {
        let content = match self {
            Overlay::QuickSelect(q) => q.view(theme, structure).map(OverlayMessage::QuickSelect),
            Overlay::Settings => todo!("Settings not yet implemented"),
            Overlay::None => panic!("Empty overlay should not be tried to be displayed"),
        };

        let dialog = container(content).padding(structure.gap);
        // .style(|theme: &iced::Theme| container::Style {
        //     background: Some(theme.palette().background),
        //     border: iced::Border {
        //         radius: 10.0.into(),
        //         ..Default::default()
        //     },
        //     ..Default::default()
        // });

        let modal = opaque(dialog);

        let backdrop = container(modal)
            .width(Fill)
            .height(Fill)
            .center(Fill)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                ..Default::default()
            });

        mouse_area(backdrop).on_press(OverlayMessage::Close).into()
    }
}
