use std::collections::BTreeSet;

use crate::{common::*, components::phosphor_icon};
use components::{
    AppearanceSection, AppearanceSectionAction, AppearanceSectionMessage, AudioSection,
    AudioSectionAction, AudioSectionMessage, ChatsSection, ChatsSectionAction, ChatsSectionMessage,
    GeneralSection, GeneralSectionAction, GeneralSectionMessage, UpdatesSection,
    UpdatesSectionAction, UpdatesSectionMessage,
};
use deplace_core::{settings::SettingsSection, state::UserDevice};
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
    NeedsAvatar(OwnedMxcUri),
}

impl NeedsAvatarExt for SettingsMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatar(uri)
    }
}

pub enum SettingsAction {
    Run(Task<()>),
    NeedsMedia(NeedsMedia),
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

    own_display_name: Receiver<Option<String>>,
    own_avatar_url: Receiver<Option<OwnedMxcUri>>,
    own_device: Arc<UserDevice>,

    avatar_cache: AvatarCache,
}

impl ExtraHash for SettingsView {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.own_display_name.borrow().hash(state);
        self.own_avatar_url.borrow().hash(state);
    }
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

            own_display_name: state.own_display_name(),
            own_avatar_url: state.own_avatar_url(),
            own_device: state.own_device(),

            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),
        }
    }
}

impl IcedWidget<SettingsMessage, SettingsAction> for SettingsView {
    fn update(&mut self, message: SettingsMessage) -> Option<SettingsAction> {
        match message {
            SettingsMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(SettingsAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
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

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, SettingsMessage> {
        let content: iced::Element<'static, SettingsMessage> = match self.active_section {
            SettingsSection::General => self
                .general
                .view(theme, structure, help_state)
                .map(SettingsMessage::General),
            SettingsSection::Appearance => self
                .appearance
                .view(theme, structure, help_state)
                .map(SettingsMessage::Appearance),
            SettingsSection::Audio => self
                .audio
                .view(theme, structure, help_state)
                .map(SettingsMessage::Audio),
            SettingsSection::Chats => self
                .chats
                .view(theme, structure, help_state)
                .map(SettingsMessage::Chats),
            SettingsSection::Updates => self
                .updates
                .view(theme, structure, help_state)
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
                                render_profile_section(
                                    theme,
                                    structure,
                                    self.active_section,
                                    self.own_avatar_url.borrow().clone(),
                                    self.own_display_name.borrow().clone(),
                                    &self.own_device.user_id,
                                    &self.avatar_cache
                                ),
                                w::container("")
                                    .width(Length::Fill)
                                    .height(structure.divider_width)
                                    .style(move |_| ContainerStyle {
                                        background: Some(theme.border.into()),
                                        border: border::rounded(structure.divider_width / 2.0),
                                        ..Default::default()
                                    }),
                                render_section(
                                    theme,
                                    structure,
                                    SettingsSection::General,
                                    icons::sliders::FILL,
                                    self.active_section
                                ),
                                render_section(
                                    theme,
                                    structure,
                                    SettingsSection::Appearance,
                                    icons::palette::REGULAR,
                                    self.active_section
                                ),
                                render_section(
                                    theme,
                                    structure,
                                    SettingsSection::Audio,
                                    icons::headphones::FILL,
                                    self.active_section
                                ),
                                render_section(
                                    theme,
                                    structure,
                                    SettingsSection::Chats,
                                    icons::chat::FILL,
                                    self.active_section
                                ),
                                w::container("")
                                    .width(Length::Fill)
                                    .height(structure.divider_width)
                                    .style(move |_| ContainerStyle {
                                        background: Some(theme.border.into()),
                                        border: border::rounded(structure.divider_width / 2.0),
                                        ..Default::default()
                                    }),
                                render_section(
                                    theme,
                                    structure,
                                    SettingsSection::Updates,
                                    icons::arrows_clockwise::FILL,
                                    self.active_section
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

fn render_section(
    theme: Theme,
    structure: Structure,
    section: SettingsSection,
    icon: &'static str,
    active_section: SettingsSection,
) -> Element<'static, SettingsMessage> {
    let is_selected = active_section == section;

    w::button(
        w::row![
            phosphor_icon(icon, structure.font_size * 1.2),
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
                theme.text.normal.into()
            } else {
                theme.text.dim.into()
            },
            background: is_selected.then_some(theme.solid_hover_bg.into()),
            border: Border {
                color: if is_selected || is_active {
                    theme.border.into()
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
    .into()
}

fn render_profile_section(
    theme: Theme,
    structure: Structure,
    active_section: SettingsSection,
    avatar_url: Option<OwnedMxcUri>,
    display_name: Option<String>,
    user_id: &OwnedUserId,
    avatar_cache: &AvatarCache,
) -> Element<'static, SettingsMessage> {
    let is_selected = active_section == SettingsSection::Profile;

    let fallback = move || {
        text_icon(
            display_name
                .unwrap_or(user_id.to_string())
                .chars()
                .next()
                .unwrap_or('?'),
            structure.server_column.icon_size,
            structure.server_column.icon_size / 2.0,
            DePlaceColor::from(user_id.as_ref()).into(),
        )
    };

    w::button(
        w::row![render_avatar(
            avatar_url,
            structure.server_column.icon_size,
            structure.server_column.icon_size / 2.0,
            fallback,
            avatar_cache
        )]
        .spacing(structure.small_gap)
        .align_y(Alignment::Center),
    )
    .on_press_maybe(if !is_selected {
        Some(SettingsMessage::ChangeSection(SettingsSection::Profile))
    } else {
        None
    })
    .padding(structure.small_gap)
    .style(move |_, status| {
        let is_active = status.active();
        ButtonStyle {
            text_color: if is_selected || status.active() {
                theme.text.normal.into()
            } else {
                theme.text.dim.into()
            },
            background: is_selected.then_some(theme.solid_hover_bg.into()),
            border: Border {
                color: if is_selected || is_active {
                    theme.border.into()
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
    .into()
}
