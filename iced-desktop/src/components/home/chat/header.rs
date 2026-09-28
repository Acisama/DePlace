use std::collections::BTreeSet;

use deplace_core::{settings::NameDecoration, state::PresenceMap};
use enumset::EnumSet;
use iced::Alignment;
use macros::iced_cache;

use crate::{common::*, components::phosphor_icon};

use super::sidebar::SidebarState;

#[derive(Debug, Clone)]
pub enum HeaderMessage {
    TogglePins,
    ToggleSearch,
    ToggleList,
    NeedsAvatar(OwnedMxcUri),
    LeaveCall,
    JoinCall,
}

impl NeedsAvatarExt for HeaderMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        HeaderMessage::NeedsAvatar(uri)
    }
}

pub enum HeaderAction {
    NeedsMedia(NeedsMedia),
    TogglePins,
    ToggleSearch,
    ToggleList,
    JoinCall,
    LeaveCall,
}

#[iced_cache(Clone)]
pub struct Header {
    state: AppState,
    avatar_cache: AvatarCache,

    #[hash]
    sidebar_state: EnumSet<SidebarState>,

    room_watchers: RoomWatchers,
    room_id: OwnedRoomId,

    name_decoration: Receiver<NameDecoration>,

    presence_map: Receiver<PresenceMap>,
}

impl ExtraHash for Header {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name_decoration.borrow().hash(state);
    }
}

impl Header {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        let id = room.room_id().to_owned();
        Self {
            avatar_cache: state.avatar_cache().clone(),
            presence_map: state.presence_map().clone(),

            sidebar_state: EnumSet::new(),

            name_decoration: state.settings().name_decoration.watch(),

            room_watchers: state
                .room_watchers(hashing::hash_room_default(id.clone()))
                .clone(),
            room_id: id.clone(),
            state: state.clone(),

            avatar_states_for_hash: BTreeSet::new(),
        }
    }
}

impl IcedWidget<HeaderMessage, HeaderAction> for Header {
    fn update(&mut self, msg: HeaderMessage) -> Option<HeaderAction> {
        match msg {
            HeaderMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(HeaderAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
            HeaderMessage::TogglePins => {
                if self.sidebar_state.contains(SidebarState::Pins) {
                    self.sidebar_state.remove(SidebarState::Pins);
                } else {
                    self.sidebar_state.insert(SidebarState::Pins);
                }
                Some(HeaderAction::TogglePins)
            }
            HeaderMessage::ToggleSearch => {
                if self.sidebar_state.contains(SidebarState::Search) {
                    self.sidebar_state.remove(SidebarState::Search);
                } else {
                    self.sidebar_state.insert(SidebarState::Search);
                }
                Some(HeaderAction::ToggleSearch)
            }
            HeaderMessage::ToggleList => {
                if self.sidebar_state.contains(SidebarState::MemberList) {
                    self.sidebar_state.remove(SidebarState::MemberList);
                } else {
                    self.sidebar_state.insert(SidebarState::MemberList);
                }
                Some(HeaderAction::ToggleList)
            }
            HeaderMessage::JoinCall => Some(HeaderAction::JoinCall),
            HeaderMessage::LeaveCall => Some(HeaderAction::LeaveCall),
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, HeaderMessage> {
        let Some(room) = self.room_watchers.get_room(&self.room_id) else {
            return w::Space::new().into();
        };

        let icon_size = structure.header.icon_size;

        let render_icon = |color, hover_color, icon, message, tooltip| {
            themed_tooltip(
                w::button(phosphor_icon(icon, structure.header.button_size))
                    .on_press(message)
                    .style(move |_, status| ButtonStyle {
                        background: None,
                        text_color: if status.active() { hover_color } else { color },
                        ..Default::default()
                    })
                    .padding(0),
                tooltip,
                structure,
                theme,
            )
        };

        let is_in_call = room.own_user_is_in_call();

        floating_tile(
            theme,
            structure,
            w::row![
                w::row![
                    w::container(context_room_icon(
                        &room,
                        icon_size,
                        theme,
                        structure,
                        &self.presence_map.borrow(),
                        &self.avatar_cache,
                        theme.solid_bg.into()
                    ))
                    .style(move |_| ContainerStyle {
                        text_color: Some(theme.text.normal.into()),
                        ..Default::default()
                    }),
                    room.render_name_decorated(
                        structure.font_size,
                        *self.name_decoration.borrow(),
                        theme.text.dim.into()
                    ),
                ]
                .padding(structure.header.inner_icon_padding())
                .align_y(Alignment::Center)
                .spacing(structure.small_gap),
                Space::new().width(Fill),
                render_icon(
                    theme.text.dim.into(),
                    if is_in_call {
                        theme.colors.red.into()
                    } else {
                        theme.colors.green.into()
                    },
                    if is_in_call {
                        phosphor_svgs::icon::phone_disconnect::BOLD
                    } else {
                        phosphor_svgs::icon::phone::REGULAR
                    },
                    if is_in_call {
                        HeaderMessage::LeaveCall
                    } else {
                        HeaderMessage::JoinCall
                    },
                    if is_in_call {
                        "Leave Call"
                    } else {
                        "Start Voice Call"
                    }
                ),
                render_icon(
                    theme.text.dim.into(),
                    theme.colors.yellow.into(),
                    if self.sidebar_state.contains(SidebarState::Pins) {
                        phosphor_svgs::icon::push_pin::FILL
                    } else {
                        phosphor_svgs::icon::push_pin::REGULAR
                    },
                    HeaderMessage::TogglePins,
                    "Toggle Pins"
                ),
                render_icon(
                    theme.text.dim.into(),
                    theme.colors.green.into(),
                    if self.sidebar_state.contains(SidebarState::MemberList) {
                        if room.is_dm() {
                            phosphor_svgs::icon::user_circle::FILL
                        } else {
                            phosphor_svgs::icon::user_list::FILL
                        }
                    } else {
                        if room.is_dm() {
                            phosphor_svgs::icon::user_circle::REGULAR
                        } else {
                            phosphor_svgs::icon::user_list::REGULAR
                        }
                    },
                    HeaderMessage::ToggleList,
                    if room.is_dm() {
                        "Toggle User Profile"
                    } else {
                        "Toggle Member List"
                    },
                ),
            ]
            .align_y(Alignment::Center)
            .spacing(structure.gap),
        )
        .width(Fill)
        .padding(structure.header.button_padding())
        .height(structure.header.height)
        .into()
    }
}
