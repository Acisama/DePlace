use crate::common::*;
use components::{
    AppearanceSection, AppearanceSectionAction, AppearanceSectionMessage, AudioSection,
    AudioSectionAction, AudioSectionMessage, ChatsSection, ChatsSectionAction, ChatsSectionMessage,
    GeneralSection, GeneralSectionAction, GeneralSectionMessage, UpdatesSection,
    UpdatesSectionAction, UpdatesSectionMessage,
};
use deplace_core::settings::SettingsSection;
use iced::{
    Alignment::{self, Center},
    Length,
    widget::{opaque, space},
};
use macros::iced_cache;
use phosphor_svgs::icon as icons;

pub const SETTINGS_INPUT_ID: &str = "settings_input";

mod components;
mod widgets;

#[derive(Debug, Clone)]
pub enum SettingsMessage {
    ChangeSection(SettingsSection),
    Profile,
    General(GeneralSectionMessage),
    Appearance(AppearanceSectionMessage),
    Audio(AudioSectionMessage),
    Chats(ChatsSectionMessage),
    Updates(UpdatesSectionMessage),
}

pub enum SettingsAction {
    Run(Task<()>),
}

#[iced_cache(Clone)]
pub struct SettingsView {
    #[hash]
    general: GeneralSection,
    #[hash]
    appearance: AppearanceSection,
    #[hash]
    audio: AudioSection,
    #[hash]
    chats: ChatsSection,
    #[hash]
    updates: UpdatesSection,

    #[hash]
    active_section: SettingsSection,
}

impl SettingsView {
    pub fn new(state: &AppState) -> Self {
        let settings = state.settings().clone();
        Self {
            general: GeneralSection::new(settings.clone()),
            appearance: AppearanceSection::new(settings.clone()),
            audio: AudioSection::new(settings.clone()),
            chats: ChatsSection::new(settings.clone()),
            updates: UpdatesSection::new(settings.clone()),

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
            SettingsMessage::General(msg) => match self.general.update(msg)? {
                GeneralSectionAction::Run(task) => Some(SettingsAction::Run(task)),
            },
            SettingsMessage::Appearance(msg) => match self.appearance.update(msg)? {
                AppearanceSectionAction::Run(task) => Some(SettingsAction::Run(task)),
            },
            SettingsMessage::Audio(msg) => match self.audio.update(msg)? {
                AudioSectionAction::Run(task) => Some(SettingsAction::Run(task)),
            },
            SettingsMessage::Chats(msg) => match self.chats.update(msg)? {
                ChatsSectionAction::Run(task) => Some(SettingsAction::Run(task)),
            },
            SettingsMessage::Updates(msg) => match self.updates.update(msg)? {
                UpdatesSectionAction::Run(task) => Some(SettingsAction::Run(task)),
            },
            SettingsMessage::Profile => None,
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> iced::Element<'static, SettingsMessage> {
        let text_size = structure.font_size;

        let render_section = move |section: SettingsSection, icon: &'static str| {
            let is_selected = self.active_section == section;

            w::button(
                w::row![
                    phosphor_icon(icon, text_size * 1.2),
                    w::text(section.display_name())
                ]
                .spacing(structure.small_gap)
                .align_y(Alignment::Center),
            )
            .on_press_maybe(if !is_selected {
                Some(SettingsMessage::ChangeSection(section))
            } else {
                None
            })
            .padding(structure.small_gap)
            .style(move |_, status| {
                let is_active = status.active();
                ButtonStyle {
                    text_color: if is_selected || status.active() {
                        theme.text.normal
                    } else {
                        theme.text.dim
                    },
                    background: is_selected.then_some(theme.solid_hover_bg.into()),
                    border: Border {
                        color: if is_selected || is_active {
                            theme.border
                        } else {
                            Color::TRANSPARENT
                        },
                        width: structure.border_thickness,
                        radius: structure.inner_border_radius.into(),
                    },
                    ..Default::default()
                }
            })
            .width(Length::Fill)
        };

        let content: iced::Element<'static, SettingsMessage> = match self.active_section {
            SettingsSection::General => self
                .general
                .view(theme, structure)
                .map(SettingsMessage::General),
            SettingsSection::Appearance => self
                .appearance
                .view(theme, structure)
                .map(SettingsMessage::Appearance),
            SettingsSection::Audio => self
                .audio
                .view(theme, structure)
                .map(SettingsMessage::Audio),
            SettingsSection::Chats => self
                .chats
                .view(theme, structure)
                .map(SettingsMessage::Chats),
            SettingsSection::Updates => self
                .updates
                .view(theme, structure)
                .map(SettingsMessage::Updates),
            SettingsSection::Profile => w::text("test").into(),
        };

        w::column![
            space().height(Length::FillPortion(1)),
            w::row![
                space().width(Length::FillPortion(1)),
                opaque(
                    w::row![
                        floating_tile(
                            theme,
                            structure,
                            w::column![
                                render_section(SettingsSection::Profile, icons::pencil::FILL),
                                w::container("")
                                    .width(Length::Fill)
                                    .height(structure.divider_width)
                                    .style(move |_| ContainerStyle {
                                        background: Some(theme.border.into()),
                                        border: border::rounded(structure.divider_width / 2.0),
                                        ..Default::default()
                                    }),
                                render_section(SettingsSection::General, icons::sliders::FILL),
                                render_section(
                                    SettingsSection::Appearance,
                                    icons::palette::REGULAR
                                ),
                                render_section(SettingsSection::Audio, icons::headphones::FILL),
                                render_section(SettingsSection::Chats, icons::chat::FILL),
                                w::container("")
                                    .width(Length::Fill)
                                    .height(structure.divider_width)
                                    .style(move |_| ContainerStyle {
                                        background: Some(theme.border.into()),
                                        border: border::rounded(structure.divider_width / 2.0),
                                        ..Default::default()
                                    }),
                                render_section(
                                    SettingsSection::Updates,
                                    icons::arrows_clockwise::FILL
                                ),
                            ]
                            .width(structure.settings.section_column_width)
                            .height(Length::Fill)
                            .padding(structure.small_gap)
                            .spacing(structure.small_gap),
                        ),
                        w::column![
                            floating_tile(
                                theme,
                                structure,
                                weighted_text(self.active_section.display_name(), Weight::Bold)
                                    .color(theme.text.normal)
                                    .width(Fill)
                                    .height(structure.header.height)
                                    .align_y(Center)
                                    .size(structure.large_font_size)
                            )
                            .padding(padding::left(
                                (structure.header.height - structure.large_font_size) / 2.0
                            )),
                            floating_tile(
                                theme,
                                structure,
                                themed_scrollable(content, theme, structure)
                                    .height(Length::Fill)
                                    .width(Length::Fill)
                            )
                        ]
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .spacing(structure.small_gap)
                    ]
                    .spacing(structure.small_gap)
                    .width(Length::FillPortion(3)),
                ),
                space().width(Length::FillPortion(1)),
            ]
            .height(Length::FillPortion(4)),
            space().height(Length::FillPortion(1)),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
