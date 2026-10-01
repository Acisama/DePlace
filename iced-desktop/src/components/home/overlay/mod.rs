use context_menu::{ContextMenuAction, ContextMenuMessage};
use deplace_core::state::AppState;
use iced::{
    Color,
    Length::Fill,
    Task,
    widget::{container, mouse_area, operation::focus},
};
use macros::iced_cache;
use profile::{OverlayProfile, ProfileAction, ProfileMessage};
use quick_select::{QUICK_SELECT_INPUT_ID, QuickSelect, QuickSelectAction, QuickSelectMessage};
use settings::{SETTINGS_INPUT_ID, SettingsAction, SettingsMessage, SettingsView};

use crate::common::*;

mod context_menu;
mod profile;
mod quick_select;
mod settings;

pub use context_menu::{ContextMenu, ContextMenuKind};

#[iced_cache(Clone)]
pub struct Overlay {
    state: AppState,

    #[hash]
    overlay_state: Option<OverlayState>,

    membership_map: Receiver<MembershipMap>,

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
    ContextMenu(ContextMenuMessage),
}

#[derive(Clone, PartialEq, Hash)]
enum OverlayState {
    QuickSelect,
    Settings,
    Profile,
    ContextMenu(ContextMenu),
}

impl Overlay {
    pub fn new(state: &AppState) -> Self {
        let quickselect = QuickSelect::new(state);
        let settings = SettingsView::new(state);

        Self {
            state: state.clone(),

            overlay_state: None,

            membership_map: state.membership_map(),

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

    pub fn open_profile(&mut self, room_id: OwnedRoomId, user_id: OwnedUserId, bounds: Rectangle) {
        tracing::trace!("Opening profile for {:?}", user_id);
        self.overlay_state = Some(OverlayState::Profile);
        self.profile = self
            .membership_map
            .borrow()
            .get(&room_id)
            .and_then(|m| m.get(&user_id).cloned())
            .map(|member| OverlayProfile::new(&self.state, member, bounds));
    }

    pub fn open_context_menu(&mut self, menu: ContextMenu) {
        tracing::trace!("Opening context menu at {:?}", menu.position);
        self.overlay_state = Some(OverlayState::ContextMenu(menu));
    }
}

#[derive(Debug)]
pub enum OverlayAction {
    Run(Task<()>),
    Perform(Task<OverlayMessage>),
    NeedsMedia(NeedsMedia),
    ChangeRoom(Option<DePlaceRoom>),
    SetIsReplyingTo {
        item_id: String,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
    },
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
                OverlayState::ContextMenu { .. } => None,
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
            OverlayMessage::ContextMenu(message) => {
                if let Some(OverlayState::ContextMenu(menu)) = &mut self.overlay_state {
                    match menu.update(message)? {
                        ContextMenuAction::SetReplyingTo {
                            item_id,
                            room_id,
                            event_id,
                        } => {
                            self.overlay_state = None;
                            Some(OverlayAction::SetIsReplyingTo {
                                item_id,
                                room_id,
                                event_id,
                            })
                        }
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
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, OverlayMessage> {
        let Some(state) = &self.overlay_state else {
            return Space::new().into();
        };

        let backdrop: Element<'static, OverlayMessage> = match state {
            OverlayState::Profile => {
                let Some(profile) = &self.profile else {
                    return Space::new().into();
                };
                profile
                    .view(theme, structure, help_state)
                    .map(OverlayMessage::Profile)
            }
            OverlayState::ContextMenu(menu) => menu
                .view(theme, structure, help_state)
                .map(OverlayMessage::ContextMenu),
            OverlayState::QuickSelect | OverlayState::Settings => {
                let content = match state {
                    OverlayState::QuickSelect => self
                        .quickselect
                        .view(theme, structure, help_state)
                        .map(OverlayMessage::QuickSelect),
                    OverlayState::Settings => self
                        .settings
                        .view(theme, structure, help_state)
                        .map(OverlayMessage::Settings),
                    _ => Space::new().into(),
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
