use crate::common::*;
use macros::iced_cache;

pub const SETTINGS_INPUT_ID: &str = "settings_input";

#[derive(Debug, Clone)]
pub enum SettingsMessage {}

pub enum SettingsAction {}

#[iced_cache(Clone)]
pub struct SettingsView {
    settings: Settings,
}

impl SettingsView {
    pub fn new(state: &AppState) -> Self {
        Self {
            settings: state.settings().clone(),
        }
    }
}

impl IcedWidget<SettingsMessage, SettingsAction> for SettingsView {
    fn update(&mut self, message: SettingsMessage) -> Option<SettingsAction> {
        None
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, SettingsMessage> {
        Space::new().into()
    }
}
