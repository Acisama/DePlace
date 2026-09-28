use deplace_core::state::AppState;
use iced::{
    Color,
    Length::Fill,
    Task,
    widget::{container, mouse_area, operation::focus},
};
use macros::iced_cache;
use matrix_sdk::room::RoomMember;
use profile::{OverlayProfile, ProfileAction, ProfileMessage};
use quick_select::{QUICK_SELECT_INPUT_ID, QuickSelect, QuickSelectAction, QuickSelectMessage};
use settings::{SETTINGS_INPUT_ID, SettingsAction, SettingsMessage, SettingsView};

use crate::common::*;

use super::sidebar::SidebarMessage;

mod profile;
mod quick_select;
mod settings;

#[iced_cache(Clone)]
pub struct Overlay {
    state: AppState,

    #[hash]
    overlay_state: Option<OverlayState>,

    quickselect: QuickSelect,
    settings: SettingsView,
    profile: Option<OverlayProfile>,
}

impl ExtraHash for Overlay {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self.overlay_state {
            Some(OverlayState::QuickSelect) => self.quickselect.hash(state),
            Some(OverlayState::Settings) => self.settings.hash(state),
            Some(OverlayState::Profile) => self.profile.hash(state),
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub enum OverlayMessage {
    Close,
    Profile(ProfileMessage),
    QuickSelect(QuickSelectMessage),
    Settings(SettingsMessage),
    KeyboardEvent(iced::keyboard::Event),
}

#[derive(Clone, Hash, PartialEq)]
enum OverlayState {
    QuickSelect,
    Settings,
    Profile,
}

impl Overlay {
    pub fn new(state: &AppState) -> Self {
        let quickselect = QuickSelect::new(state);
        let settings = SettingsView::new(state);

        Self {
            state: state.clone(),

            overlay_state: None,

            quickselect,
            settings,
            profile: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.overlay_state.is_some()
    }

    pub fn toggle_quick_select(&mut self) -> Option<Task<()>> {
        tracing::trace!("Toggling quick select");
        if self.overlay_state == Some(OverlayState::QuickSelect) {
            self.overlay_state = None;
            Some(focus(QUICK_SELECT_INPUT_ID))
        } else {
            self.overlay_state = Some(OverlayState::QuickSelect);
            None
        }
    }

    pub fn toggle_settings(&mut self) -> Option<Task<()>> {
        tracing::trace!("Toggling settings");
        if self.overlay_state == Some(OverlayState::Settings) {
            self.overlay_state = None;
            Some(focus(SETTINGS_INPUT_ID))
        } else {
            self.overlay_state = Some(OverlayState::Settings);
            None
        }
    }

    pub fn open_profile(&mut self, member: RoomMember, bounds: Rectangle) {
        tracing::trace!("Opening profile for {:?}", member.user_id());
        self.overlay_state = Some(OverlayState::Profile);
        self.profile = Some(OverlayProfile::new(&self.state, member, bounds));
    }
}

#[derive(Debug)]
pub enum OverlayAction {
    Run(Task<()>),
    Perform(Task<OverlayMessage>),
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<DePlaceRoom>),
}

impl IcedWidget<OverlayMessage, OverlayAction> for Overlay {
    fn update(&mut self, message: OverlayMessage) -> Option<OverlayAction> {
        match message {
            OverlayMessage::Close => {
                self.overlay_state = None;
                None
            }
            OverlayMessage::QuickSelect(msg) => {
                if !matches!(self.overlay_state, Some(OverlayState::QuickSelect)) {
                    return None;
                }
                match self.quickselect.update(msg)? {
                    QuickSelectAction::ChangeRoom(room) => {
                        // TODO: Reset because quickselect is persistent
                        self.overlay_state = None;
                        Some(OverlayAction::ChangeRoom(room))
                    }
                    QuickSelectAction::Close => {
                        self.overlay_state = None;
                        None
                    }
                    QuickSelectAction::NeedsMedia(media) => Some(OverlayAction::NeedsMedia(media)),
                }
            }
            OverlayMessage::Settings(msg) => {
                if !matches!(self.overlay_state, Some(OverlayState::Settings)) {
                    return None;
                }
                match self.settings.update(msg)? {
                    SettingsAction::Run(task) => Some(OverlayAction::Run(task)),
                }
            }
            OverlayMessage::KeyboardEvent(event) => match self.overlay_state.as_ref()? {
                OverlayState::QuickSelect => {
                    match self
                        .quickselect
                        .update(QuickSelectMessage::KeyboardEvent(event))?
                    {
                        QuickSelectAction::ChangeRoom(room) => {
                            self.overlay_state = None;
                            Some(OverlayAction::ChangeRoom(room))
                        }
                        QuickSelectAction::Close => {
                            self.overlay_state = None;
                            None
                        }
                        QuickSelectAction::NeedsMedia(media) => {
                            Some(OverlayAction::NeedsMedia(media))
                        }
                    }
                }
                OverlayState::Settings => None,
                OverlayState::Profile => None,
            },
            OverlayMessage::Profile(message) => {
                if let Some(profile) = &mut self.profile {
                    match profile.update(message)? {
                        ProfileAction::NeedsMedia(media) => Some(OverlayAction::NeedsMedia(media)),
                        ProfileAction::CopyUserId(user_id) => Some(OverlayAction::Perform(
                            iced::clipboard::write(user_id.to_string())
                                .map_err(|_| tracing::error!("Failed to copy to clipboard"))
                                .map(|_| OverlayMessage::Profile(ProfileMessage::UserIdCopied)),
                        )),
                    }
                } else {
                    None
                }
            }
        }
    }

    fn view(
        &self,
        theme: crate::common::Theme,
        structure: crate::common::Structure,
    ) -> iced::Element<'static, OverlayMessage> {
        let Some(state) = &self.overlay_state else {
            return Space::new().into();
        };

        let backdrop: Element<'static, OverlayMessage> = match state {
            OverlayState::Profile => {
                let Some(profile) = &self.profile else {
                    return Space::new().into();
                };
                profile.view(theme, structure).map(OverlayMessage::Profile)
            }
            OverlayState::QuickSelect | OverlayState::Settings => {
                let content = match state {
                    OverlayState::QuickSelect => self
                        .quickselect
                        .view(theme, structure)
                        .map(OverlayMessage::QuickSelect),
                    OverlayState::Settings => self
                        .settings
                        .view(theme, structure)
                        .map(OverlayMessage::Settings),
                    OverlayState::Profile => Space::new().into(),
                };

                let dialog = container(content).padding(structure.gap);

                container(dialog)
                    .width(Fill)
                    .height(Fill)
                    .center(Fill)
                    .style(|_theme| container::Style {
                        background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.6).into()),
                        ..Default::default()
                    })
                    .into()
            }
        };

        mouse_area(backdrop)
            .on_press(OverlayMessage::Close)
            .interaction(iced::mouse::Interaction::Idle)
            .into()
    }
}
