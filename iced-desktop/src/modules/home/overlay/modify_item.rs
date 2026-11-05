use iced::{
    keyboard::{self, Key::Named},
    widget::text::IntoFragment,
};
use macros::iced_cache;
use matrix_sdk_ui::{Timeline, timeline::TimelineEventItemId};

use crate::{
    common::*, components::animated_button::AnimatedButton, modules::home::chat::MessageEvent,
};

#[derive(Clone, Debug)]
pub enum ModifyItemMessage {
    None,
    Delete { reason: Option<String> },
    Pin(OwnedEventId),
    Unpin(OwnedEventId),
    Close,
    KeyboardEvent(keyboard::Event),
}

pub enum ModifyItemAction {
    Run(Task<()>),
    Perform(Task<ModifyItemMessage>),
    Close,
}

#[derive(Clone, Hash, Debug, PartialEq)]
enum ModifyItemKind {
    Delete(TimelineEventItemId),
    Pin(OwnedEventId),
    Unpin(OwnedEventId),
}

#[iced_cache(Clone, Debug)]
pub struct ModifyItem {
    timeline: Arc<Timeline>,
    event: Arc<MessageEvent>,

    #[hash]
    identifier: TimelineEventItemId,
    #[hash]
    kind: ModifyItemKind,
}

impl PartialEq for ModifyItem {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier && self.kind == other.kind
    }
}

impl ModifyItem {
    pub fn delete(
        timeline: Arc<Timeline>,
        identifier: TimelineEventItemId,
        event: Arc<MessageEvent>,
    ) -> Self {
        Self {
            timeline,
            identifier: identifier.clone(),
            event,
            kind: ModifyItemKind::Delete(identifier),
        }
    }

    pub fn pin(
        timeline: Arc<Timeline>,
        identifier: TimelineEventItemId,
        event_id: OwnedEventId,
        event: Arc<MessageEvent>,
        is_pinned: bool,
    ) -> Self {
        Self {
            timeline,
            identifier,
            event,
            kind: if is_pinned {
                ModifyItemKind::Unpin(event_id)
            } else {
                ModifyItemKind::Pin(event_id)
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
                let identifier = self.identifier.clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = timeline.redact(&identifier, reason.as_deref()).await {
                        tracing::error!("Failed to redact event: {}", e);
                    }
                })))
            }
            ModifyItemMessage::Pin(event_id) => {
                let room = self.timeline.room().clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = room.pin_event(&event_id).await {
                        tracing::error!("Failed to pin event: {}", e);
                    }
                })))
            }
            ModifyItemMessage::Unpin(event_id) => {
                let room = self.timeline.room().clone();
                Some(ModifyItemAction::Run(Task::future(async move {
                    if let Err(e) = room.unpin_event(&event_id).await {
                        tracing::error!("Failed to unpin event: {}", e);
                    }
                })))
            }
            ModifyItemMessage::KeyboardEvent(event) => {
                if let iced::keyboard::Event::KeyPressed { key, .. } = event {
                    if key == Named(keyboard::key::Named::Enter) {
                        match &self.kind {
                            ModifyItemKind::Delete(_) => {
                                return Some(ModifyItemAction::Perform(Task::done(
                                    ModifyItemMessage::Delete { reason: None },
                                )));
                            }
                            ModifyItemKind::Pin(event_id) => {
                                return Some(ModifyItemAction::Perform(Task::done(
                                    ModifyItemMessage::Pin(event_id.clone()),
                                )));
                            }
                            ModifyItemKind::Unpin(event_id) => {
                                return Some(ModifyItemAction::Perform(Task::done(
                                    ModifyItemMessage::Unpin(event_id.clone()),
                                )));
                            }
                        }
                    }

                    if key == Named(keyboard::key::Named::Escape) {
                        return Some(ModifyItemAction::Close);
                    }
                }
                None
            }
        }
    }

    fn view(&self) -> iced::Element<'static, ModifyItemMessage> {
        let theme = theme();
        let structure = structure();

        let room_id = self.timeline.room().room_id().to_owned();

        let content = self
            .event
            .view(room_id, theme, structure, true, help_state(), 0, false)
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
            ModifyItemKind::Delete(_) => (
                "Redact Message",
                "Are you sure you want to redact this message?",
                [
                    cancel_button,
                    render_dialogue_button(
                        structure,
                        "Redact",
                        ModifyItemMessage::Delete { reason: None },
                        Some(theme.error_blended()),
                        theme.colors.error,
                        theme.colors.error,
                        theme.solid_bg,
                        theme.colors.error,
                    ),
                ],
            ),
            ModifyItemKind::Pin(event_id) => (
                "Pin Message",
                "Are you sure you want to pin this message?",
                [
                    cancel_button,
                    render_dialogue_button(
                        structure,
                        "Put a pin in it",
                        ModifyItemMessage::Pin(event_id.clone()),
                        Some(theme.pins_blended()),
                        theme.colors.pins,
                        theme.colors.pins,
                        theme.solid_bg,
                        theme.colors.pins,
                    ),
                ],
            ),
            ModifyItemKind::Unpin(event_id) => (
                "Unpin Message",
                "Are you sure you want to unpin this message?",
                [
                    cancel_button,
                    render_dialogue_button(
                        structure,
                        "Remove the pin",
                        ModifyItemMessage::Unpin(event_id.clone()),
                        Some(theme.pins_blended()),
                        theme.colors.pins,
                        theme.colors.pins,
                        theme.solid_bg,
                        theme.colors.pins,
                    ),
                ],
            ),
        };

        render_floating_dialogue(theme, structure, heading, subheading, content, buttons)
    }
}

#[allow(clippy::too_many_arguments)]
fn render_dialogue_button<'a, Message: Clone + 'a>(
    structure: Structure,
    label: impl IntoFragment<'a>,
    message: Message,
    bg_color: Option<DePlaceColor>,
    hover_bg_color: DePlaceColor,
    text_color: DePlaceColor,
    hover_text_color: DePlaceColor,
    border_color: DePlaceColor,
) -> AnimatedButton<Message, Text<'a>> {
    animated_button(w::text(label).width(Fill).center())
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

fn render_floating_dialogue<'a, Message: Clone + 'a, Content: DePlaceWidget<Message> + 'a>(
    theme: Theme,
    structure: Structure,
    heading: impl IntoFragment<'a>,
    subheading: impl IntoFragment<'a>,
    content: Content,
    buttons: impl IntoIterator<Item = AnimatedButton<Message, Text<'a>>>,
) -> Element<'a, Message> {
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
            w::container(content)
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
            w::Row::with_children(buttons).spacing(structure.small_gap)
        ]
        .spacing(structure.small_gap)
        .padding(structure.gap),
    )
    .width(structure.modify_menu_width)
    .into()
}
