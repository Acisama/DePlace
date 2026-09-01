use crate::common::*;
use deplace_core::matrix_api::{EncryptionUpgradeResult, recover_client_encryption};
use iced::widget::button::Status;

#[derive(Debug, Clone)]
pub enum VerificationMessage {
    RecoveryKeyChanged(String),
    Checking,
    Submit,
    VerificationFailed(String),
    VerificationSucceeded,
}

impl From<EncryptionUpgradeResult> for VerificationMessage {
    fn from(res: EncryptionUpgradeResult) -> Self {
        match res {
            EncryptionUpgradeResult::Verified => VerificationMessage::VerificationSucceeded,
            EncryptionUpgradeResult::Error(err) => VerificationMessage::VerificationFailed(err),
        }
    }
}

#[derive(Debug)]
pub enum VerificationAction {
    None,
    Run(Task<EncryptionUpgradeResult>),
    Success(AppState),
}

pub struct Verification {
    app_state: AppState,
    recovery_key: String,
    state: GenericState<AppState>,

    current_check: Option<iced::task::Handle>,
}

impl Hash for Verification {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.recovery_key.hash(state);
        self.state.hash(state);
    }
}

impl Verification {
    pub fn new(app_state: AppState) -> Self {
        Self {
            app_state,
            recovery_key: String::new(),
            state: GenericState::Error("Recovery key cannot be empty".into()),
            current_check: None,
        }
    }

    pub fn check_key_format(&mut self) -> bool {
        if self.recovery_key.is_empty() {
            self.state = GenericState::Error("Recovery key cannot be empty".into());
            return false;
        }

        let parts: Vec<&str> = self.recovery_key.split_whitespace().collect();

        if parts.len() != 12 {
            self.state = GenericState::Error("Recovery key must be 12 words long".into());
            return false;
        }

        for part in &parts {
            if part.len() != 4 {
                self.state = GenericState::Error("Each word must be 4 characters long".into());
                return false;
            }
        }

        self.state = GenericState::Ready;
        true
    }
}

impl IcedWidget<VerificationMessage, VerificationAction> for Verification {
    fn update(&mut self, message: VerificationMessage) -> VerificationAction {
        match message {
            VerificationMessage::RecoveryKeyChanged(key) => {
                self.recovery_key = key;
                self.check_key_format();
                VerificationAction::None
            }
            VerificationMessage::Submit => {
                if !self.check_key_format() {
                    return VerificationAction::None;
                }
                self.state = GenericState::Checking;

                let state = self.app_state.clone();
                let recovery_key = self.recovery_key.clone();

                let (task, handle) =
                    Task::future(recover_client_encryption(state, recovery_key)).abortable();
                self.current_check = Some(handle.abort_on_drop());

                VerificationAction::Run(task)
            }
            VerificationMessage::Checking => {
                self.state = GenericState::Checking;
                VerificationAction::None
            }
            VerificationMessage::VerificationFailed(error) => {
                self.state = GenericState::Error(error);
                VerificationAction::None
            }
            VerificationMessage::VerificationSucceeded => {
                self.state = GenericState::Success(self.app_state.clone());
                VerificationAction::Success(self.app_state.clone())
            }
        }
    }

    fn view(&self, theme: Theme, structure: Structure) -> Element<'static, VerificationMessage> {
        let input: iced::widget::TextInput<'static, VerificationMessage> =
            text_input("Es9X xxxx xxxx...", &self.recovery_key, theme, structure)
                .width(Fill)
                .secure(true)
                .on_input(VerificationMessage::RecoveryKeyChanged)
                .on_submit(VerificationMessage::Submit);

        let status: iced::widget::text::Rich<'static, (), VerificationMessage> = self.state.text(
            "Successfully verified",
            "Key is formatted correctly",
            &theme.colors,
            &structure,
        );

        let recovery_button = w::button(w::text("Log in").width(Fill).center())
            .width(Fill)
            .style(move |_, status| w::button::Style {
                background: Some(match status {
                    Status::Disabled => theme.colors.muted.into(),
                    Status::Active => theme.accent.into(),
                    _ => theme.accent.scale_alpha(0.5).into(),
                }),
                text_color: theme.background,
                border: Border {
                    radius: structure.semi_border_radius().into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .on_press_maybe(self.state.ready().then_some(VerificationMessage::Submit));

        let tile = floating_tile(
            theme,
            structure,
            w::column![
                weighted_text("Verification", iced::font::Weight::ExtraBold)
                    .size(structure.large_font_size)
                    .color(theme.accent)
                    .width(Fill)
                    .center(),
                w::column![
                    w::text("Recovery Key")
                        .size(structure.font_size)
                        .color(theme.text.dim),
                    Space::new().height(structure.small_gap),
                    input,
                ],
                status,
                recovery_button
            ]
            .width(structure.authentification.width)
            .spacing(structure.gap)
            .padding(structure.gap),
        );

        w::container(tile).center(Fill).into()
    }
}
