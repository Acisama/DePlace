use std::sync::Arc;

use deplace_core::{
    matrix_api::{EncryptionUpgradeResult, recover_client_encryption},
    state::AppState,
};
use gpui::{
    ClickEvent, Context, Entity, EventEmitter, KeyDownEvent, Render, WeakEntity, Window, div,
    prelude::*,
};
use gpui_component::{
    Disableable, StyledExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{Input, InputState},
};

use crate::{components::floating_tile, things::DeplaceThings};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum KeyAquiryStage {
    #[default]
    VerificationChoice,
    RecoveryKeyInput(Entity<InputState>),
    EmojiVerification,
}

#[derive(Debug, Clone)]
pub enum KeyAquiryEvent {
    Back,
    Verified,
    Error,
    Canceled,
}

pub struct KeyAquiryView {
    tokio_rt: Arc<tokio::runtime::Runtime>,
    state: AppState,
    stage: KeyAquiryStage,
    is_submitting: bool,
    error_message: Option<String>,
}

impl EventEmitter<KeyAquiryEvent> for KeyAquiryView {}

impl KeyAquiryView {
    pub fn new(tokio_rt: Arc<tokio::runtime::Runtime>, state: AppState) -> Self {
        Self {
            tokio_rt,
            state,
            stage: KeyAquiryStage::VerificationChoice,
            is_submitting: false,
            error_message: None,
        }
    }

    fn on_back_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(KeyAquiryEvent::Back);
    }

    fn on_substage_back_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stage = KeyAquiryStage::VerificationChoice;
        self.error_message = None;
        cx.notify();
    }

    fn on_verify_recovery_key_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input_state = cx.new(|cx| InputState::new(window, cx));
        self.stage = KeyAquiryStage::RecoveryKeyInput(input_state);
        self.error_message = None;
        cx.notify();
    }

    fn on_verify_emojis_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stage = KeyAquiryStage::EmojiVerification;
        cx.notify();
    }

    fn on_submit_recovery_key_click(
        &mut self,
        _: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_recovery_key(window, cx);
    }

    fn submit_recovery_key(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let recovery_key = match &self.stage {
            KeyAquiryStage::RecoveryKeyInput(input_state) => {
                input_state.read(cx).text().to_string()
            }
            _ => return,
        };

        if self.is_submitting || recovery_key.is_empty() {
            return;
        }

        self.is_submitting = true;
        self.error_message = None;
        cx.notify();

        let state = self.state.clone();
        let tokio_rt = Arc::clone(&self.tokio_rt); // <-- Clone the runtime handle

        cx.spawn(
            async move |this: WeakEntity<KeyAquiryView>, cx: &mut gpui::AsyncApp| {
                // Execute the Matrix SDK code on Tokio, then await the JoinHandle!
                let result = tokio_rt
                    .spawn(async move {
                        recover_client_encryption(state, recovery_key).await
                    })
                    .await
                    .unwrap_or_else(|e| {
                        tracing::error!("Tokio task failed or panicked: {:?}", e);
                        EncryptionUpgradeResult::Error(e.to_string())
                    });

                tracing::info!("Recovered encryption for this client");

                let _ = cx.update(|cx| {
                    let _ = this.update(cx, |view, cx| {
                        view.is_submitting = false;
                        match result {
                            EncryptionUpgradeResult::Verified => {
                                cx.emit(KeyAquiryEvent::Verified);
                            }
                            EncryptionUpgradeResult::Error(_) => {
                                view.error_message = Some(
                                    "Failed to recover encryption. Please check your key and try again."
                                        .to_string(),
                                );
                            }
                        }
                        cx.notify();
                    });
                });
            },
        )
        .detach();
    }

    fn render_verification_choice(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        floating_tile(theme, structure)
            .paddings(structure.gap)
            .flex_col()
            .gap(structure.gap * 2)
            .w(gpui::px(360.0))
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(
                        div()
                            .absolute()
                            .left(structure.gap)
                            .top(structure.gap)
                            .child(
                                h_flex()
                                    .id("verify-back")
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .text_color(theme.text.dim)
                                    .text_xs()
                                    .font_bold()
                                    .hover(|style| style.text_decoration_1())
                                    .child("⟵ back")
                                    .on_click(cx.listener(Self::on_back_click)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_2xl()
                            .font_extrabold()
                            .text_color(theme.accent)
                            .child("Verify Session"),
                    ),
            )
            .child(
                div()
                    .text_center()
                    .text_sm()
                    .text_color(theme.text.dim)
                    .child("Choose a method to verify this device:"),
            )
            .child(
                div()
                    .flex_col()
                    .flex()
                    .gap(structure.gap)
                    .child({
                        let variant = ButtonCustomVariant::new(cx)
                            .color(theme.accent)
                            .hover(theme.accent)
                            .active(theme.accent);

                        Button::new("verify-recovery-key")
                            .label("Verify using Recovery Key")
                            .custom(variant)
                            .w_full()
                            .on_click(cx.listener(Self::on_verify_recovery_key_click))
                    })
                    .child(
                        Button::new("verify-emojis")
                            .label("Verify using Emojis")
                            .w_full()
                            .on_click(cx.listener(Self::on_verify_emojis_click)),
                    ),
            )
    }

    fn render_recovery_key_input(
        &mut self,
        input_state: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        let recovery_key_text = input_state.read(cx).text().to_string();
        let is_empty = recovery_key_text.trim().is_empty();

        floating_tile(theme, structure)
            .paddings(structure.gap)
            .flex_col()
            .gap(structure.gap * 2)
            .w(gpui::px(360.0))
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(
                        div()
                            .absolute()
                            .left(structure.gap)
                            .top(structure.gap)
                            .child(
                                h_flex()
                                    .id("recovery-back")
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .text_color(theme.text.dim)
                                    .text_xs()
                                    .font_bold()
                                    .hover(|style| style.text_decoration_1())
                                    .child("⟵ back")
                                    .on_click(cx.listener(Self::on_substage_back_click)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_2xl()
                            .font_extrabold()
                            .text_color(theme.accent)
                            .child("Recovery Key"),
                    ),
            )
            .child(
                div()
                    .text_center()
                    .text_sm()
                    .text_color(theme.text.dim)
                    .child("Enter your recovery key to decrypt your session:"),
            )
            .child(
                div()
                    .flex_col()
                    .flex()
                    .gap(structure.gap)
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "enter" {
                            this.submit_recovery_key(window, cx);
                        }
                    }))
                    .child(Input::new(input_state))
                    .when_some(self.error_message.clone(), |this, err| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(gpui::red())
                                .text_center()
                                .child(err),
                        )
                    })
                    .child({
                        let variant = ButtonCustomVariant::new(cx)
                            .color(theme.accent)
                            .hover(theme.accent)
                            .active(theme.accent);

                        Button::new("submit-recovery-key")
                            .label(if self.is_submitting {
                                "Recovering..."
                            } else {
                                "Recover Session"
                            })
                            .custom(variant)
                            .disabled(self.is_submitting || is_empty)
                            .w_full()
                            .on_click(cx.listener(Self::on_submit_recovery_key_click))
                    }),
            )
    }

    fn render_emoji_verification(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        floating_tile(theme, structure)
            .paddings(structure.gap)
            .flex_col()
            .gap(structure.gap * 2)
            .w(gpui::px(360.0))
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(
                        div()
                            .absolute()
                            .left(structure.gap)
                            .top(structure.gap)
                            .child(
                                h_flex()
                                    .id("emoji-back")
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .text_color(theme.text.dim)
                                    .text_xs()
                                    .font_bold()
                                    .hover(|style| style.text_decoration_1())
                                    .child("⟵ back")
                                    .on_click(cx.listener(Self::on_substage_back_click)),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_2xl()
                            .font_extrabold()
                            .text_color(theme.accent)
                            .child("Emoji Verification"),
                    ),
            )
            .child(
                div()
                    .text_center()
                    .text_sm()
                    .text_color(theme.text.dim)
                    .child("Emoji verification screen placeholder."),
            )
    }
}

impl Render for KeyAquiryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match &self.stage {
            KeyAquiryStage::VerificationChoice => {
                self.render_verification_choice(cx).into_any_element()
            }
            KeyAquiryStage::RecoveryKeyInput(input_state) => {
                let input_state = input_state.clone();
                self.render_recovery_key_input(&input_state, cx)
                    .into_any_element()
            }
            KeyAquiryStage::EmojiVerification => {
                self.render_emoji_verification(cx).into_any_element()
            }
        };

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(content)
    }
}
