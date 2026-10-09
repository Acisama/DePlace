use deplace_core::state::UserDevice;
use iced::Alignment;
use macros::{iced_cache, iced_icon};
use std::collections::BTreeSet;

use crate::{common::*, components::home::SidebarHelpKey};

#[derive(Debug, Clone)]
pub enum AccountMessage {
    NeedsAvatar(OwnedMxcUri),
    OpenSettings,
    HelpHover(Option<HelpKey>),
}

impl NeedsAvatarExt for AccountMessage {
    fn needs_avatar(uri: OwnedMxcUri) -> Self {
        Self::NeedsAvatar(uri)
    }
}

pub enum AccountAction {
    NeedsMedia(NeedsMedia),
    OpenSettings,
    HelpHover(Option<HelpKey>),
}

#[iced_cache(Clone, Debug)]
pub struct AccountView {
    own_display_name: Receiver<Option<String>>,
    own_avatar_url: Receiver<Option<OwnedMxcUri>>,
    own_device: Arc<UserDevice>,

    avatar_cache: AvatarCache,
}

impl ExtraHash for AccountView {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.own_display_name.borrow().hash(state);
        self.own_avatar_url.borrow().hash(state);
    }
}

impl AccountView {
    pub fn new(state: &AppState) -> Self {
        Self {
            own_display_name: state.own_display_name(),
            own_avatar_url: state.own_avatar_url(),
            own_device: state.own_device(),

            avatar_cache: state.avatar_cache().clone(),
            avatar_states_for_hash: BTreeSet::new(),
        }
    }
}

impl IcedWidget<AccountMessage, AccountAction> for AccountView {
    fn update(&mut self, message: AccountMessage) -> Option<AccountAction> {
        match message {
            AccountMessage::NeedsAvatar(uri) => {
                self.avatar_states_for_hash.insert(uri.clone());
                Some(AccountAction::NeedsMedia(NeedsMedia::avatar(uri)))
            }
            AccountMessage::OpenSettings => Some(AccountAction::OpenSettings),
            AccountMessage::HelpHover(key) => Some(AccountAction::HelpHover(key)),
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, AccountMessage> {
        let help_view = HelpView::new(help_state, theme, AccountMessage::HelpHover);

        let header_height = structure.header.height;
        let button_size = structure.header.button_size;

        let user_id = self.own_device.user_id.clone();
        let display_name = self
            .own_display_name
            .borrow()
            .clone()
            .unwrap_or(user_id.to_string());
        let avatar_url = self.own_avatar_url.borrow().clone();

        let initial = display_name.chars().next().unwrap_or('?');

        let color = DePlaceColor::from(user_id.as_str()).into();

        let fallback = move || text_icon(initial, button_size, button_size * 0.5, color);

        let content = floating_tile(
            theme,
            structure,
            w::container(
                w::row![
                    render_avatar(
                        avatar_url,
                        button_size,
                        button_size * 0.5,
                        fallback,
                        &self.avatar_cache
                    ),
                    render_name(display_name, structure.font_size, color),
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
            .into()
    }
}
