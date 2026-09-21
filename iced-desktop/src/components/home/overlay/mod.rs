use deplace_core::state::AppState;
use iced::{
    Color,
    Length::Fill,
    Task,
    widget::{container, mouse_area, operation::focus},
};
use macros::iced_cache;
use quick_select::{QUICK_SELECT_INPUT_ID, QuickSelect, QuickSelectAction, QuickSelectMessage};
use settings::{SETTINGS_INPUT_ID, SettingsMessage, SettingsView};

use crate::common::*;

mod quick_select;
mod settings;

#[iced_cache(Clone)]
pub struct Overlay {
    #[hash]
    state: Option<OverlayState>,

    quickselect: QuickSelect,
    settings: SettingsView,
}

impl ExtraHash for Overlay {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self.state {
            Some(OverlayState::QuickSelect) => self.quickselect.hash(state),
            Some(OverlayState::Settings) => self.settings.hash(state),
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub enum OverlayMessage {
    Close,
    QuickSelect(QuickSelectMessage),
    Settings(SettingsMessage),
    KeyboardEvent(iced::keyboard::Event),
}

#[derive(Hash, Clone, PartialEq)]
enum OverlayState {
    QuickSelect,
    Settings,
}

impl Overlay {
    pub fn new(state: &AppState) -> Self {
        let quickselect = QuickSelect::new(state);
        let settings = SettingsView::new(state);

        Self {
            state: None,

            quickselect,
            settings,
        }
    }

    pub fn is_open(&self) -> bool {
        self.state.is_some()
    }

    pub fn toggle_quick_select(&mut self) -> Option<Task<()>> {
        tracing::trace!("Toggling quick select");
        if self.state == Some(OverlayState::QuickSelect) {
            self.state = None;
            Some(focus(QUICK_SELECT_INPUT_ID))
        } else {
            self.state = Some(OverlayState::QuickSelect);
            None
        }
    }

    pub fn toggle_settings(&mut self) -> Option<Task<()>> {
        tracing::trace!("Toggling settings");
        if self.state == Some(OverlayState::Settings) {
            self.state = None;
            Some(focus(SETTINGS_INPUT_ID))
        } else {
            self.state = Some(OverlayState::Settings);
            None
        }
    }
}

#[derive(Debug)]
pub enum OverlayAction {
    Run(Task<()>),
    Perform(Task<OverlayMessage>),
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<Room>),
}

impl IcedWidget<OverlayMessage, OverlayAction> for Overlay {
    fn update(&mut self, message: OverlayMessage) -> Option<OverlayAction> {
        match message {
            OverlayMessage::Close => {
                self.state = None;
                None
            }
            OverlayMessage::QuickSelect(msg) => {
                if !matches!(self.state, Some(OverlayState::QuickSelect)) {
                    return None;
                }
                match self.quickselect.update(msg)? {
                    QuickSelectAction::ChangeRoom(room) => {
                        // TODO: Reset because quickselect is persistent
                        self.state = None;
                        Some(OverlayAction::ChangeRoom(room))
                    }
                    QuickSelectAction::Close => {
                        self.state = None;
                        None
                    }
                    QuickSelectAction::NeedsMedia(media) => Some(OverlayAction::NeedsMedia(media)),
                }
            }
            OverlayMessage::Settings(msg) => {
                if !matches!(self.state, Some(OverlayState::Settings)) {
                    return None;
                }
                match self.settings.update(msg)? {}
            }
            OverlayMessage::KeyboardEvent(event) => match self.state.as_ref()? {
                OverlayState::QuickSelect => {
                    match self
                        .quickselect
                        .update(QuickSelectMessage::KeyboardEvent(event))?
                    {
                        QuickSelectAction::ChangeRoom(room) => {
                            self.state = None;
                            Some(OverlayAction::ChangeRoom(room))
                        }
                        QuickSelectAction::Close => {
                            self.state = None;
                            None
                        }
                        QuickSelectAction::NeedsMedia(media) => {
                            Some(OverlayAction::NeedsMedia(media))
                        }
                    }
                }
                OverlayState::Settings => None,
            },
        }
    }

    fn view(
        &self,
        theme: crate::common::Theme,
        structure: crate::common::Structure,
    ) -> iced::Element<'static, OverlayMessage> {
        let content = match &self.state {
            Some(OverlayState::QuickSelect) => self
                .quickselect
                .view(theme, structure)
                .map(OverlayMessage::QuickSelect),
            Some(OverlayState::Settings) => self
                .settings
                .view(theme, structure)
                .map(OverlayMessage::Settings),
            None => Space::new().into(),
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

        let backdrop = container(dialog)
            .width(Fill)
            .height(Fill)
            .center(Fill)
            .style(|_theme| container::Style {
                background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                ..Default::default()
            });

        mouse_area(backdrop)
            .on_press(OverlayMessage::Close)
            .interaction(iced::mouse::Interaction::Idle)
            .into()
    }
}
