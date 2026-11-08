use deplace_core::state::UserDevice;
use iced::Alignment;
use macros::{iced_cache, iced_icon};

use crate::{common::*, components::help_mode::SidebarHelpKey};

#[derive(Debug, Clone)]
pub enum AccountMessage {
    OpenSettings,
    HelpHover(Option<HelpKey>),
}

impl HelpOverMessage for AccountMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        AccountMessage::HelpHover(help)
    }
}

pub enum AccountAction {
    OpenSettings,
    HelpHover(Option<HelpKey>),
}

#[iced_cache(Clone, Debug)]
pub struct AccountView {
    profile_store: Arc<ProfileStore>,

    own_display_name: Receiver<Option<String>>,
    own_avatar: Receiver<Avatar>,
    own_device: Arc<UserDevice>,
}

impl ExtraHash for AccountView {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.own_display_name.borrow().hash(state);
        self.own_avatar.borrow().hash(state);
    }
}

impl AccountView {
    pub fn new(state: &AppState) -> Self {
        Self {
            profile_store: state.profile_store(),

            own_display_name: state.own_display_name(),
            own_avatar: state.own_small_avatar(),
            own_device: state.own_device(),
        }
    }
}

impl IcedWidget<AccountMessage, AccountAction> for AccountView {
    fn update(&mut self, message: AccountMessage) -> Option<AccountAction> {
        match message {
            AccountMessage::OpenSettings => Some(AccountAction::OpenSettings),
            AccountMessage::HelpHover(key) => Some(AccountAction::HelpHover(key)),
        }
    }

    fn view(&self) -> iced::Element<'static, AccountMessage> {
        let theme = theme();
        let structure = structure();

        let help_view = HelpView::new(theme);

        let header_height = structure.header.height;
        let button_size = structure.header.button_size;

        let user_id = self.own_device.user_id.clone();
        let display_name = self
            .own_display_name
            .borrow()
            .clone()
            .unwrap_or(user_id.to_string());

        let initial = display_name.chars().next().unwrap_or('?');

        let color = DePlaceColor::from(user_id.as_str()).into();

        let fallback = move || text_icon(initial, button_size, button_size * 0.5, color);
        let presence = self.profile_store.get_presence(&user_id);

        let avatar = render_avatar(
            button_size,
            button_size * 0.5,
            fallback,
            self.own_avatar.borrow().clone(),
        );

        let content = floating_tile(
            theme,
            structure,
            w::container(
                w::row![
                    render_presence_badge(
                        avatar,
                        presence,
                        theme,
                        structure,
                        theme.solid_bg.into(),
                    ),
                    w::column![
                        render_name(display_name, structure.font_size, color),
                        w::text(presence.display_string())
                            .color(theme.text.dim)
                            .size(structure.small_font_size)
                    ]
                    .spacing(structure.small_gap / 4.0),
                    Space::new().width(Fill),
                    help_view.call(
                        HelpKey::Sidebar(SidebarHelpKey::SettingsButton),
                        "Settings button, press to open settings",
                        w::button(iced_icon!(gear, regular, button_size))
                            .style(move |_, status| ButtonStyle {
                                background: None,
                                text_color: if status.active() {
                                    theme.text.normal.into()
                                } else {
                                    theme.text.dim.into()
                                },
                                border: border::rounded(structure.inner_border_radius),
                                ..Default::default()
                            })
                            .padding(0.0)
                            .on_press(AccountMessage::OpenSettings)
                    ),
                ]
                .align_y(Alignment::Center)
                .spacing(structure.small_gap),
            )
            .width(structure.sidebar.width)
            .height(structure.header.height)
            .padding((header_height - button_size) / 2.0),
        );

        help_view
            .call(
                HelpKey::Sidebar(SidebarHelpKey::Account),
                "Your account",
                content,
            )
            .radius(structure.outer_border_radius)
            .into()
    }
}
