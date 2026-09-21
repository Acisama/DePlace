use crate::common::*;
use deplace_core::settings::SettingsSection;
use iced::{Length, widget::space};
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
        w::column![
            space().height(Length::FillPortion(1)),
            w::row![
                space().width(Length::FillPortion(1)),
                w::row![
                    floating_tile(
                        theme,
                        structure,
                        Space::new()
                            .height(Fill)
                            .width(structure.settings.section_column_width),
                    ),
                    w::column![
                        floating_tile(
                            theme,
                            structure,
                            space().width(Fill).height(structure.header.height)
                        ),
                        floating_tile(theme, structure, space().width(Fill).height(Fill))
                    ]
                    .spacing(structure.small_gap)
                ]
                .spacing(structure.small_gap)
                .width(Length::FillPortion(3)),
                space().width(Length::FillPortion(1)),
            ]
            .height(Length::FillPortion(3)),
            space().height(Length::FillPortion(1)),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
