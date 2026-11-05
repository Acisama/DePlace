use deplace_core::{
    settings::NameDecoration,
    state::media_fetcher::{MediaProcessor, ProgressCompleteness},
};
use enumset::EnumSet;
use iced::Alignment;
use macros::iced_cache;

use crate::{
    common::*,
    components::{help_mode::HeaderHelpKey, phosphor_icon},
    modules::home::{CONNECT, DISCONNECT},
};

use super::sidebar::SidebarState;

#[derive(Debug, Clone)]
pub enum HeaderMessage {
    TogglePins,
    ToggleSearch,
    ToggleList,
    LeaveCall,
    JoinCall,
    HelpHover(Option<HelpKey>),
    OpenHelpMenu,
}

impl HelpOverMessage for HeaderMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        HeaderMessage::HelpHover(help)
    }
}

pub enum HeaderAction {
    TogglePins,
    ToggleSearch,
    ToggleList,
    JoinCall,
    LeaveCall,
    HelpHover(Option<HelpKey>),
    OpenHelpMenu,
}

#[iced_cache(Clone)]
pub struct Header {
    #[hash]
    sidebar_state: EnumSet<SidebarState>,

    room: DePlaceRoom,

    name_decoration: Receiver<NameDecoration>,

    media_processor: MediaProcessor,
}

impl ExtraHash for Header {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name_decoration.borrow().hash(state);

        self.media_processor.current_download().hash(state);
        self.media_processor.current_compression().hash(state);

        self.room.hash_selective(state, true, true, true);
    }
}

impl Header {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        Self {
            sidebar_state: EnumSet::new(),

            name_decoration: state.settings().name_decoration.watch(),

            room: room.clone(),

            media_processor: state.media_processor(),
        }
    }
}

impl IcedWidget<HeaderMessage, HeaderAction> for Header {
    fn update(&mut self, msg: HeaderMessage) -> Option<HeaderAction> {
        match msg {
            HeaderMessage::OpenHelpMenu => Some(HeaderAction::OpenHelpMenu),
            HeaderMessage::HelpHover(help) => Some(HeaderAction::HelpHover(help)),
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

    fn view(&self) -> Element<'static, HeaderMessage> {
        let theme = theme();
        let structure = structure();

        let help_view = HelpView::new(theme);

        let icon_size = structure.header.icon_size;
        let button_size = structure.header.button_size;

        let render_icon = |color, hover_color, icon, message, tooltip| {
            themed_tooltip_text(
                animated_button(phosphor_icon(icon, button_size))
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

        let is_in_call = self.room.live().own_user_device_is_in_call();

        let download = self.media_processor.current_download();
        let compression = self.media_processor.current_compression();

        let content =
            floating_tile(
                theme,
                structure,
                w::row![
                    w::row![
                    w::container(help_view.call(
                        HelpKey::Header(HeaderHelpKey::HeaderIcon),
                        "The icon of the active room, shows other user's icon in dms",
                        self.room.render_simple_icon(
                            if self.room.is_dm() {
                                button_size
                            } else {
                                icon_size
                            },
                            theme,
                            structure,
                            theme.solid_bg.into(),
                        )
                    ).radius(if self.room.is_dm() { button_size * 0.5 } else {icon_size * 0.25 }))
                    .style(move |_| ContainerStyle {
                        text_color: Some(theme.text.normal.into()),
                        ..Default::default()
                    }),
                    help_view.call(
                        HelpKey::Header(HeaderHelpKey::HeaderName),
                        "The name of the active room, shows the other user's display name in dms",
                        self.room.render_name_decorated(
                            structure.font_size,
                            *self.name_decoration.borrow(),
                            theme.text.dim.into()
                        ),
                    ),
                ].align_y(Alignment::Center)
                    .padding(if self.room.is_dm() {
                        0.0
                    } else {
                        structure.header.inner_icon_padding()
                    })
                    .align_y(Alignment::Center)
                    .spacing(structure.small_gap),
                    Space::new().width(Fill),
                    download.map(|progress| {
                        let color = theme.accent.into();
                        let element = match progress.completeness() {
                            ProgressCompleteness::NotStarted => pie_progress(0.0, button_size, color, structure.border_thickness, None),
                            ProgressCompleteness::InProgress(v) => pie_progress(v, button_size, color, structure.border_thickness, None),
                            ProgressCompleteness::Completed => pie_progress(0.0, button_size, color, structure.border_thickness, Some(phosphor_svgs::icon::check_circle::FILL)),
                            ProgressCompleteness::Failed => pie_progress(0.0, button_size, theme.colors.error.into(), structure.border_thickness, Some(phosphor_svgs::icon::x_circle::FILL)),
                        };
                        help_view.call(HelpKey::Header(HeaderHelpKey::CurrentDownload), "The status of the current download", element)
                    }),
                    compression.map(|progress| {
                        let color = theme.colors.compression.into();
                        let element = match progress.completeness() {
                            ProgressCompleteness::NotStarted => pie_progress(0.0, button_size, color, structure.border_thickness, None),
                            ProgressCompleteness::InProgress(v) => pie_progress(v, button_size, color, structure.border_thickness, None),
                            ProgressCompleteness::Completed => pie_progress(0.0, button_size, color, structure.border_thickness, Some(phosphor_svgs::icon::check_circle::FILL)),
                            ProgressCompleteness::Failed => pie_progress(0.0, button_size, theme.colors.error.into(), structure.border_thickness, Some(phosphor_svgs::icon::x_circle::FILL)),
                        };
                        help_view.call(HelpKey::Header(HeaderHelpKey::CurrentCompression), "The status of the current compression task", element)
                    }),
                    help_view.call(
                        HelpKey::Header(HeaderHelpKey::CallButton),
                        "Call button, press it to start or leave a voice call",
                        render_icon(
                            theme.text.dim.into(),
                            if is_in_call {
                                theme.colors.red.into()
                            } else {
                                theme.colors.green.into()
                            },
                            if is_in_call {
                                DISCONNECT
                            } else {
                                CONNECT
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
                        )
                    ).radius(button_size * 0.25),
                    help_view.call(
                        HelpKey::Header(HeaderHelpKey::OpenPins),
                        "Button to toggle pins in the current room",
                        render_icon(
                            theme.text.dim.into(),
                            theme.colors.pins.into(),
                            if self.sidebar_state.contains(SidebarState::Pins) {
                                phosphor_svgs::icon::push_pin::FILL
                            } else {
                                phosphor_svgs::icon::push_pin::REGULAR
                            },
                            HeaderMessage::TogglePins,
                            "Toggle Pins"
                        )
                    ).radius(button_size * 0.25),
                    help_view.call(
                        HelpKey::Header(HeaderHelpKey::OpenMemberList),
                        "Button to toggle the member list in the current room or the other user's profile if in a DM",
                        render_icon(
                            theme.text.dim.into(),
                            theme.colors.green.into(),
                            if self.sidebar_state.contains(SidebarState::MemberList) {
                                if self.room.is_dm() {
                                    phosphor_svgs::icon::user_circle::FILL
                                } else {
                                    phosphor_svgs::icon::user_list::FILL
                                }
                            } else {
                                if self.room.is_dm() {
                                    phosphor_svgs::icon::user_circle::REGULAR
                                } else {
                                    phosphor_svgs::icon::user_list::REGULAR
                                }
                            },
                            HeaderMessage::ToggleList,
                            if self.room.is_dm() {
                                "Toggle User Profile"
                            } else {
                                "Toggle Member List"
                            },
                        )
                    ).radius(button_size * 0.25),
                    help_view.call(
                        HelpKey::Header(HeaderHelpKey::HelpButton),
                        "Help button, press to open this menu",
                        render_icon(
                            theme.text.dim.into(),
                            theme.accent.into(),
                            phosphor_svgs::icon::question::REGULAR,
                            HeaderMessage::OpenHelpMenu,
                            "Open Help Menu"
                        )
                    ).radius(button_size * 0.25)
                ]
                .align_y(Alignment::Center)
                .spacing(structure.gap),
            )
            .width(Fill)
            .padding(structure.header.button_padding())
            .height(structure.header.height);

        help_view
            .call(
                HelpKey::Header(HeaderHelpKey::Header),
                "The chat header",
                content,
            )
            .radius(structure.outer_border_radius)
            .into()
    }
}
