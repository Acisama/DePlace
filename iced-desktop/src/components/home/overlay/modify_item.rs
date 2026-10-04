use iced::widget::text::IntoFragment;
use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::{common::*, components::home::chat::MessageEvent};

#[derive(Clone, Debug)]
pub enum ModifyItemMessage {
    None,
    Delete { reason: Option<String> },
    Pin,
    Unpin,
    Close,
}

pub enum ModifyItemAction {
    Run(Task<()>),
    Close,
}

#[derive(Clone, Copy, Hash, Debug, PartialEq)]
enum ModifyItemKind {
    Delete,
    Pin,
    Unpin,
}

#[iced_cache(Clone, Debug)]
pub struct ModifyItem {
    timeline: Arc<Timeline>,
    event: Arc<MessageEvent>,

    #[hash]
    event_id: OwnedEventId,
    #[hash]
    kind: ModifyItemKind,
}

impl PartialEq for ModifyItem {
    fn eq(&self, other: &Self) -> bool {
        self.event_id == other.event_id && self.kind == other.kind
    }
}

impl ModifyItem {
    pub fn delete(
        timeline: Arc<Timeline>,
        event_id: OwnedEventId,
        event: Arc<MessageEvent>,
    ) -> Self {
        Self {
            timeline,
            event_id,
            event,
            kind: ModifyItemKind::Delete,
        }
    }

    pub fn pin(
        timeline: Arc<Timeline>,
        event_id: OwnedEventId,
        event: Arc<MessageEvent>,
        is_pinned: bool,
    ) -> Self {
        Self {
            timeline,
            event_id,
            event,
            kind: if is_pinned {
                ModifyItemKind::Unpin
            } else {
                ModifyItemKind::Pin
            },
        }
    }
}

impl IcedWidget<ModifyItemMessage, ModifyItemAction> for ModifyItem {
    fn update(&mut self, message: ModifyItemMessage) -> Option<ModifyItemAction> {
        match message {
            ModifyItemMessage::None => None,
            ModifyItemMessage::Close => Some(ModifyItemAction::Close),
            ModifyItemMessage::Delete { reason } => {
                let timeline = self.timeline.clone();
                let event_id = self.event_id.clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = timeline
                        .redact(
                            &matrix_sdk_ui::timeline::TimelineEventItemId::EventId(event_id),
                            reason.as_deref(),
                        )
                        .await
                    {
                        tracing::error!("Failed to redact event: {}", e);
                    }
                })))
            }
            ModifyItemMessage::Pin => {
                let room = self.timeline.room().clone();
                let event_id = self.event_id.clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = room.pin_event(&event_id).await {
                        tracing::error!("Failed to pin event: {}", e);
                    }
                })))
            }
            ModifyItemMessage::Unpin => {
                let room = self.timeline.room().clone();
                let event_id = self.event_id.clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = room.unpin_event(&event_id).await {
                        tracing::error!("Failed to unpin event: {}", e);
                    }
                })))
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, ModifyItemMessage> {
        let room_id = self.timeline.room().room_id().to_owned();

        let content = self
            .event
            .view(room_id, theme, structure, true, help_state, 0, false)
            .map(|_| ModifyItemMessage::None);

        let cancel_button = render_dialogue_button(
            structure,
            "Cancel",
            ModifyItemMessage::Close,
            Some(theme.background),
            theme.text.dim,
            theme.text.dim,
            theme.solid_bg,
            theme.border,
        );

        let (heading, subheading, buttons) = match &self.kind {
            ModifyItemKind::Delete => (
                "Redact Message",
                "Are you sure you want to redact this message?",
                [
                    cancel_button,
                    render_dialogue_button(
                        structure,
                        "Redact",
                        ModifyItemMessage::Delete { reason: None },
                        Some(theme.solid_bg.blend(theme.colors.error, 0.1)),
                        theme.colors.error,
                        theme.colors.error,
                        theme.solid_bg,
                        theme.colors.error,
                    ),
                ],
            ),
            ModifyItemKind::Pin => (
                "Pin Message",
                "Are you sure you want to pin this message?",
                [
                    cancel_button,
                    render_dialogue_button(
                        structure,
                        "Put a pin in",
                        ModifyItemMessage::Pin,
                        Some(theme.solid_bg.blend(theme.colors.yellow, 0.1)),
                        theme.colors.yellow,
                        theme.colors.yellow,
                        theme.solid_bg,
                        theme.colors.yellow,
                    ),
                ],
            ),
            ModifyItemKind::Unpin => (
                "Unpin Message",
                "Are you sure you want to unpin this message?",
                [
                    cancel_button,
                    render_dialogue_button(
                        structure,
                        "Remove the pin",
                        ModifyItemMessage::Unpin,
                        Some(theme.solid_bg.blend(theme.colors.yellow, 0.1)),
                        theme.colors.yellow,
                        theme.colors.yellow,
                        theme.solid_bg,
                        theme.colors.yellow,
                    ),
                ],
            ),
        };

        render_floating_dialogue(theme, structure, heading, subheading, content, buttons)
    }
}

#[allow(clippy::too_many_arguments)]
fn render_dialogue_button<'a, T: Clone + 'a>(
    structure: Structure,
    label: impl IntoFragment<'a>,
    message: T,
    bg_color: Option<DePlaceColor>,
    hover_bg_color: DePlaceColor,
    text_color: DePlaceColor,
    hover_text_color: DePlaceColor,
    border_color: DePlaceColor,
) -> w::Button<'a, T> {
    w::button(w::text(label).width(Fill).center())
        .style(move |_, status| ButtonStyle {
            background: if status.active() {
                Some(hover_bg_color.into())
            } else {
                bg_color.map(|c| c.into())
            },
            text_color: if status.active() {
                hover_text_color.into()
            } else {
                text_color.into()
            },
            border: Border {
                color: if status.active() {
                    hover_bg_color.into()
                } else {
                    border_color.into()
                },
                radius: structure.inner_border_radius.into(),
                width: structure.border_thickness,
            },
            ..Default::default()
        })
        .padding(structure.small_gap)
        .width(Fill)
        .on_press(message)
}

fn render_floating_dialogue<'a, T: Clone + 'a>(
    theme: Theme,
    structure: Structure,
    heading: impl IntoFragment<'a>,
    subheading: impl IntoFragment<'a>,
    content: impl Into<Element<'a, T>>,
    buttons: impl IntoIterator<Item = w::Button<'a, T>>,
) -> iced::Element<'a, T> {
    floating_tile(
        theme,
        structure,
        w::column![
            weighted_text(heading, Weight::Bold)
                .size(structure.large_font_size)
                .color(theme.text.normal),
            w::text(subheading)
                .size(structure.font_size)
                .color(theme.text.dim),
            w::container(content.into())
                .style(move |_| ContainerStyle {
                    background: Some(theme.solid_bg.into()),
                    border: Border {
                        color: theme.border.into(),
                        width: structure.border_thickness,
                        radius: structure.inner_border_radius.into(),
                    },
                    ..Default::default()
                })
                .padding(structure.small_gap),
            Space::new(),
            w::Row::with_children(buttons.into_iter().map(|b| b.into()))
                .spacing(structure.small_gap)
        ]
        .spacing(structure.small_gap)
        .padding(structure.gap),
    )
    .width(structure.modify_menu_width)
    .into()
}
