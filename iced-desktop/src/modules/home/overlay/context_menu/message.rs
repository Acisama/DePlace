use std::{hash::Hash, sync::Arc};

use deplace_core::{structure::Structure, theme::Theme};
use iced::{Alignment, Element};
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId};
use matrix_sdk_ui::{Timeline, timeline::TimelineEventItemId};

use crate::common::*;

use crate::components::equal_width::equal_width;
use crate::components::phosphor_icon;
use crate::modules::home::{COPY_ICON, PIN_ICON, REDACT_ICON, REPLY_ICON, SOURCE_ICON, UNPIN_ICON};
use crate::modules::home::{EDIT_ICON, chat::MessageEvent};

#[derive(Clone, Debug)]
pub enum MessageMessage {
    SetReplyingTo(OwnedEventId),
    Delete,
    Pin {
        event_id: OwnedEventId,
        is_pinned: bool,
    },
    Edit(OwnedEventId),
    CopyText(Arc<String>),
}

#[derive(Clone, Debug)]
pub struct MessageContextMenu {
    pub timeline: Arc<Timeline>,
    pub event: Arc<MessageEvent>,
    pub room_id: OwnedRoomId,
    pub identifier: TimelineEventItemId,
    pub item_id: String,

    pub is_pinned: bool,

    pub can_edit: Option<OwnedEventId>,
    pub can_reply: Option<OwnedEventId>,
    pub can_pin: Option<OwnedEventId>,
    pub can_redact: bool,
    pub copy_text: Option<Arc<String>>,
    pub source: Option<Arc<String>>,
}

impl PartialEq for MessageContextMenu {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier
    }
}

impl Hash for MessageContextMenu {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

impl MessageContextMenu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        timeline: Arc<Timeline>,
        event: Arc<MessageEvent>,
        room_id: OwnedRoomId,
        identifier: TimelineEventItemId,
        item_id: String,
        is_pinned: bool,
        can_edit: Option<OwnedEventId>,
        can_reply: Option<OwnedEventId>,
        can_pin: Option<OwnedEventId>,
        can_redact: bool,
        copy_text: Option<Arc<String>>,
        source: Option<Arc<String>>,
    ) -> Self {
        Self {
            timeline,
            event,
            room_id,
            identifier,
            item_id,
            is_pinned,
            can_edit,
            can_reply,
            can_pin,
            can_redact,
            copy_text,
            source,
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, MessageMessage> {
        let mut buttons = Vec::new();

        if let Some(event_id) = &self.can_edit {
            buttons.push(render_context_menu_button(
                structure,
                theme.text.dim,
                theme.text.normal,
                theme.solid_hover_bg,
                EDIT_ICON,
                "Edit this message",
                MessageMessage::Edit(event_id.clone()),
            ));
        }

        if let Some(event_id) = &self.can_reply {
            buttons.push(render_context_menu_button(
                structure,
                theme.text.dim,
                theme.text.normal,
                theme.solid_hover_bg,
                REPLY_ICON,
                "Reply to this message",
                MessageMessage::SetReplyingTo(event_id.clone()),
            ));
        }

        if let Some(text) = &self.copy_text {
            buttons.push(render_context_menu_button(
                structure,
                theme.text.dim,
                theme.text.normal,
                theme.solid_hover_bg,
                COPY_ICON,
                "Copy text",
                MessageMessage::CopyText(text.clone()),
            ));
        }

        if let Some(source) = &self.source {
            buttons.push(render_context_menu_button(
                structure,
                theme.text.dim,
                theme.text.normal,
                theme.solid_hover_bg,
                SOURCE_ICON,
                "Copy source",
                MessageMessage::CopyText(source.clone()),
            ));
        }

        if let Some(event_id) = &self.can_pin {
            buttons.push(render_context_menu_button(
                structure,
                theme.colors.pins,
                theme.solid_bg,
                theme.colors.pins,
                if self.is_pinned { UNPIN_ICON } else { PIN_ICON },
                if self.is_pinned {
                    "Unpin this message"
                } else {
                    "Pin this message"
                },
                MessageMessage::Pin {
                    event_id: event_id.clone(),
                    is_pinned: self.is_pinned,
                },
            ));
        }

        if self.can_redact {
            buttons.push(render_context_menu_button(
                structure,
                theme.colors.red,
                theme.solid_bg,
                theme.colors.red,
                REDACT_ICON,
                "Redact this message",
                MessageMessage::Delete,
            ));
        }

        if buttons.is_empty() {
            Space::new().boxed()
        } else {
            w::container(equal_width(buttons))
                .padding(structure.small_gap)
                .boxed()
        }
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
    message: MessageMessage,
) -> w::Button<'static, MessageMessage, Row<Element<'static, MessageMessage>>> {
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
        border: border::rounded(structure.semi_large_border_radius()),
        ..Default::default()
    })
    .on_press(message)
}
