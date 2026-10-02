use iced::Alignment;
use macros::iced_cache;
use matrix_sdk_ui::Timeline;

use crate::{
    common::*,
    components::{equal_width::equal_width, phosphor_icon},
};

use super::ModifyItem;

#[derive(Clone, Debug)]
pub enum MessageMessage {
    SetReplyingTo,
    Delete,
    Pin(bool),
    Edit,
}

#[derive(Clone, Debug)]
pub enum ContextMenuMessage {
    Message {
        timeline: Arc<Timeline>,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
        item_id: String,
        message: MessageMessage,
    },
}

pub enum ContextMenuAction {
    SetReplyingTo {
        item_id: String,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
    },
    OpenModifyItem(ModifyItem),
}

#[iced_cache(Clone, Debug, PartialEq)]
pub struct ContextMenu {
    pub position: Point,
    #[hash]
    pub kind: ContextMenuKind,
}

impl ExtraHash for ContextMenu {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.position.x.to_bits().hash(state);
        self.position.y.to_bits().hash(state);
    }
}

#[derive(Clone, Debug)]
pub enum ContextMenuKind {
    Message {
        timeline: Arc<Timeline>,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
        item_id: String,

        is_pinned: bool,

        can_edit: bool,
        can_reply: bool,
        can_pin: bool,
        can_redact: bool,
    },
}

impl Hash for ContextMenuKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ContextMenuKind::Message { .. } => 0.hash(state),
        }
    }
}

impl PartialEq for ContextMenuKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                ContextMenuKind::Message { event_id, .. },
                ContextMenuKind::Message {
                    event_id: event_id2,
                    ..
                },
            ) => event_id == event_id2,
        }
    }
}

impl IcedWidget<ContextMenuMessage, ContextMenuAction> for ContextMenu {
    fn update(&mut self, message: ContextMenuMessage) -> Option<ContextMenuAction> {
        match message {
            ContextMenuMessage::Message {
                timeline,
                event_id,
                room_id,
                item_id,
                message,
            } => match message {
                MessageMessage::SetReplyingTo => Some(ContextMenuAction::SetReplyingTo {
                    item_id,
                    room_id,
                    event_id,
                }),
                MessageMessage::Delete => Some(ContextMenuAction::OpenModifyItem(
                    ModifyItem::delete(timeline, event_id),
                )),
                MessageMessage::Pin(is_pinned) => Some(ContextMenuAction::OpenModifyItem(
                    ModifyItem::pin(timeline, event_id, is_pinned),
                )),
                // TODO
                MessageMessage::Edit => None,
            },
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        _help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, ContextMenuMessage> {
        const SIZE: f32 = 200.0;

        let position = self.position;
        let kind = self.kind.clone();

        w::responsive(move |size| {
            let content = match &kind {
                ContextMenuKind::Message {
                    timeline,
                    room_id,
                    event_id,
                    item_id,

                    is_pinned,

                    can_edit,
                    can_reply,
                    can_pin,
                    can_redact,
                } => render_message_context_menu(
                    theme,
                    structure,
                    timeline.clone(),
                    room_id.clone(),
                    event_id.clone(),
                    item_id.clone(),
                    *is_pinned,
                    *can_edit,
                    *can_reply,
                    *can_pin,
                    *can_redact,
                ),
            };

            let left = position.x.min(size.width - SIZE).max(0.0);
            let top = position.y.min(size.height - SIZE).max(0.0);

            w::container(w::container(content).style(move |_| ContainerStyle {
                background: Some(theme.solid_bg.into()),
                border: Border {
                    color: theme.border.into(),
                    width: structure.border_thickness,
                    radius: structure.inner_border_radius.into(),
                },
                ..Default::default()
            }))
            .width(Fill)
            .height(Fill)
            .padding(Padding {
                top,
                left,
                right: 0.0,
                bottom: 0.0,
            })
        })
        .into()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_message_context_menu(
    theme: Theme,
    structure: Structure,
    timeline: Arc<Timeline>,
    room_id: OwnedRoomId,
    event_id: OwnedEventId,
    item_id: String,
    is_pinned: bool,
    can_edit: bool,
    can_reply: bool,
    can_pin: bool,
    can_redact: bool,
) -> Element<'static, ContextMenuMessage> {
    let mut buttons = Vec::new();

    let message = move |message| ContextMenuMessage::Message {
        timeline: timeline.clone(),
        room_id: room_id.clone(),
        event_id: event_id.clone(),
        item_id: item_id.clone(),
        message,
    };

    if can_edit {
        buttons.push(
            render_context_menu_button(
                structure,
                theme.text.dim,
                theme.text.normal,
                theme.solid_hover_bg,
                phosphor_svgs::icon::pencil_simple::BOLD,
                "Edit this message",
                message(MessageMessage::Edit),
            )
            .into(),
        );
    }

    if can_reply {
        buttons.push(
            render_context_menu_button(
                structure,
                theme.text.dim,
                theme.text.normal,
                theme.solid_hover_bg,
                phosphor_svgs::icon::arrow_bend_up_left::BOLD,
                "Reply to this message",
                message(MessageMessage::SetReplyingTo),
            )
            .into(),
        );
    }

    if can_pin {
        buttons.push(
            render_context_menu_button(
                structure,
                theme.colors.yellow,
                theme.solid_bg,
                theme.colors.yellow,
                if is_pinned {
                    phosphor_svgs::icon::push_pin_slash::BOLD
                } else {
                    phosphor_svgs::icon::push_pin::BOLD
                },
                if is_pinned {
                    "Unpin this message"
                } else {
                    "Pin this message"
                },
                message(MessageMessage::Pin(is_pinned)),
            )
            .into(),
        );
    }

    if can_redact {
        buttons.push(
            render_context_menu_button(
                structure,
                theme.colors.red,
                theme.solid_bg,
                theme.colors.red,
                phosphor_svgs::icon::trash::BOLD,
                "Redact this message",
                message(MessageMessage::Delete),
            )
            .into(),
        );
    }

    if buttons.is_empty() {
        Space::new().into()
    } else {
        w::container(equal_width(buttons))
            .padding(structure.small_gap)
            .into()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_context_menu_button(
    structure: Structure,
    text_color: DePlaceColor,
    hover_text_color: DePlaceColor,
    hover_bg_color: DePlaceColor,
    icon: &'static str,
    label: &'static str,
    message: ContextMenuMessage,
) -> w::Button<'static, ContextMenuMessage> {
    w::button(
        w::row![
            phosphor_icon(icon, structure.font_size),
            w::text(label).size(structure.font_size)
        ]
        .spacing(structure.small_gap)
        .align_y(Alignment::Center),
    )
    .padding(structure.small_gap)
    .style(move |_, status| ButtonStyle {
        background: status.active().then_some(hover_bg_color.into()),
        text_color: if status.active() {
            hover_text_color.into()
        } else {
            text_color.into()
        },
        border: border::rounded(structure.semi_border_radius()),
        ..Default::default()
    })
    .on_press(message)
}
