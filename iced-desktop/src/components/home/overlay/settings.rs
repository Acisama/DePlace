use crate::common::*;
use deplace_core::settings::SettingsSection;
use macros::iced_cache;

pub const SETTINGS_INPUT_ID: &str = "settings_input";

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    ChangeSection(SettingsSection),
}

pub enum SettingsAction {}

#[iced_cache(Clone)]
pub struct SettingsView {
    settings: Settings,

    active_section: SettingsSection,
}

impl SettingsView {
    pub fn new(state: &AppState) -> Self {
        Self {
            settings: state.settings().clone(),

            active_section: SettingsSection::default(),
        }
    }
}

impl IcedWidget<SettingsMessage, SettingsAction> for SettingsView {
    fn update(&mut self, message: SettingsMessage) -> Option<SettingsAction> {
        match message {
            SettingsMessage::ChangeSection(section) => {
                self.active_section = section;
                None
            }
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, SettingsMessage> {
        w::row![floating_tile(
            theme,
            structure,
            Space::new().height(20.0).height(20.0)
        )]
        .into()
    }
}
