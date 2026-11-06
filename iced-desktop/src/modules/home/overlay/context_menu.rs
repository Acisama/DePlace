use deplace_core::room::call::CallMember;
use iced::Alignment;
use macros::iced_cache;
use matrix_sdk_ui::{Timeline, timeline::TimelineEventItemId};

use crate::{
    common::*,
    components::{equal_width::equal_width, phosphor_icon},
    modules::home::{EDIT_ICON, PIN_ICON, REDACT_ICON, REPLY_ICON, UNPIN_ICON, chat::MessageEvent},
};

use super::ModifyItem;

#[derive(Clone, Debug)]
pub enum MessageMessage {
    SetReplyingTo(OwnedEventId),
    Delete,
    Pin {
        event_id: OwnedEventId,
        is_pinned: bool,
    },
    Edit(OwnedEventId),
}

#[derive(Clone, Debug)]
pub enum ParticipantMessage {
    /// The new playback volume slider position, in percent (0..=400)
    Volume(f32),
    /// Mute or unmute them; muting silences, unmuting restores normal
    /// volume
    Muted(bool),
}

#[derive(Clone, Debug)]
pub enum ContextMenuMessage {
    Message {
        timeline: Arc<Timeline>,
        event: Arc<MessageEvent>,
        room_id: OwnedRoomId,
        identifier: TimelineEventItemId,
        item_id: String,
        message: MessageMessage,
    },
    Participant {
        member: CallMember,
        message: ParticipantMessage,
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
        event: Arc<MessageEvent>,
        room_id: OwnedRoomId,
        identifier: TimelineEventItemId,
        item_id: String,

        is_pinned: bool,

        can_edit: Option<OwnedEventId>,
        can_reply: Option<OwnedEventId>,
        can_pin: Option<OwnedEventId>,
        can_redact: bool,
    },
    /// One call participant: their playback volume and mute state.
    Participant { member: CallMember },
}

impl Hash for ContextMenuKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ContextMenuKind::Message { .. } => 0.hash(state),
            // The member handle holds the live volume and mute state,
            // the menu reads them on every redraw instead of being
            // invalidated by them
            ContextMenuKind::Participant { member } => {
                1.hash(state);
                member.member.user_id().hash(state);
                member.device_id.hash(state);
            }
        }
    }
}

impl PartialEq for ContextMenuKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                ContextMenuKind::Message {
                    identifier: identifier1,
                    ..
                },
                ContextMenuKind::Message {
                    identifier: identifier2,
                    ..
                },
            ) => identifier1 == identifier2,
            (
                ContextMenuKind::Participant { member: member1 },
                ContextMenuKind::Participant { member: member2 },
            ) => {
                member1.member.user_id() == member2.member.user_id()
                    && member1.device_id == member2.device_id
            }
            _ => false,
        }
    }
}

impl IcedWidget<ContextMenuMessage, ContextMenuAction> for ContextMenu {
    fn update(&mut self, message: ContextMenuMessage) -> Option<ContextMenuAction> {
        match message {
            ContextMenuMessage::Message {
                timeline,
                event,
                identifier,
                room_id,
                item_id,
                message,
            } => match message {
                MessageMessage::SetReplyingTo(event_id) => Some(ContextMenuAction::SetReplyingTo {
                    item_id,
                    room_id,
                    event_id,
                }),
                MessageMessage::Delete => Some(ContextMenuAction::OpenModifyItem(
                    ModifyItem::delete(timeline, identifier, event),
                )),
                MessageMessage::Pin {
                    event_id,
                    is_pinned,
                } => Some(ContextMenuAction::OpenModifyItem(ModifyItem::pin(
                    timeline, identifier, event_id, event, is_pinned,
                ))),
                // TODO
                MessageMessage::Edit(_) => None,
            },
            ContextMenuMessage::Participant { member, message } => {
                match message {
                    // How loud they play for us, straight into the
                    // handle: the audio pump picks it up on the next
                    // frame, no message loop wake needed. Sliding to
                    // 0% mutes them, sliding anywhere above unmutes
                    // them again.
                    ParticipantMessage::Volume(percent) => {
                        let handle = member.handle();
                        handle.set_playback_volume(percent / 100.0);
                        handle.set_playback_muted(percent <= 0.0);
                    }
                    // The playback volume stays where it is: unmuting
                    // returns to whatever they played at before muting
                    ParticipantMessage::Muted(muted) => {
                        member.handle().set_playback_muted(muted);
                    }
                }
                None
            }
        }
    }

    fn view(&self) -> iced::Element<'static, ContextMenuMessage> {
        const SIZE: f32 = 200.0;

        let theme = theme();
        let structure = structure();

        let position = self.position;
        let kind = self.kind.clone();

        w::responsive(move |size| {
            let content = match &kind {
                ContextMenuKind::Message {
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
                } => render_message_context_menu(
                    theme,
                    structure,
                    timeline.clone(),
                    event.clone(),
                    room_id.clone(),
                    identifier.clone(),
                    item_id.clone(),
                    *is_pinned,
                    can_edit.clone(),
                    can_reply.clone(),
                    can_pin.clone(),
                    *can_redact,
                ),
                ContextMenuKind::Participant { member } => {
                    render_participant_context_menu(structure, member.clone())
                }
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
        .boxed()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_message_context_menu(
    theme: Theme,
    structure: Structure,
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
) -> Element<'static, ContextMenuMessage> {
    let mut buttons = Vec::new();

    let message = move |message| ContextMenuMessage::Message {
        timeline: timeline.clone(),
        event: event.clone(),
        room_id: room_id.clone(),
        identifier: identifier.clone(),
        item_id: item_id.clone(),
        message,
    };

    if let Some(event_id) = can_edit {
        buttons.push(render_context_menu_button(
            structure,
            theme.text.dim,
            theme.text.normal,
            theme.solid_hover_bg,
            EDIT_ICON,
            "Edit this message",
            message(MessageMessage::Edit(event_id)),
        ));
    }

    if let Some(event_id) = can_reply {
        buttons.push(render_context_menu_button(
            structure,
            theme.text.dim,
            theme.text.normal,
            theme.solid_hover_bg,
            REPLY_ICON,
            "Reply to this message",
            message(MessageMessage::SetReplyingTo(event_id)),
        ));
    }

    if let Some(event_id) = can_pin {
        buttons.push(render_context_menu_button(
            structure,
            theme.colors.pins,
            theme.solid_bg,
            theme.colors.pins,
            if is_pinned { UNPIN_ICON } else { PIN_ICON },
            if is_pinned {
                "Unpin this message"
            } else {
                "Pin this message"
            },
            message(MessageMessage::Pin {
                event_id,
                is_pinned,
            }),
        ));
    }

    if can_redact {
        buttons.push(render_context_menu_button(
            structure,
            theme.colors.red,
            theme.solid_bg,
            theme.colors.red,
            REDACT_ICON,
            "Redact this message",
            message(MessageMessage::Delete),
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

#[allow(clippy::too_many_arguments)]
fn render_context_menu_button(
    structure: Structure,
    text_color: DePlaceColor,
    hover_text_color: DePlaceColor,
    hover_bg_color: DePlaceColor,
    icon: &'static str,
    label: &'static str,
    message: ContextMenuMessage,
) -> w::Button<'static, ContextMenuMessage, Row<Element<'static, ContextMenuMessage>>> {
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

/// The menu for one call participant
///
///  A slider for their playback volume and a mute toggle, maybe
/// should be a togglebox
///
/// Both live values live in the member's call handle, the menu reads
/// them fresh on every redraw so slider and toggle always agree.
fn render_participant_context_menu(
    structure: Structure,
    member: CallMember,
) -> Element<'static, ContextMenuMessage> {
    let handle = member.handle();
    let percent = (handle.playback_volume() * 100.0).round();
    let muted = handle.playback_muted();

    let slider_member = member.clone();
    let toggler_member = member.clone();

    w::column![
        w::text(format!("Volume: {}%", percent as u32)).size(structure.font_size),
        w::slider(0.0..=400.0, percent, move |value| {
            ContextMenuMessage::Participant {
                member: slider_member.clone(),
                message: ParticipantMessage::Volume(value),
            }
        })
        .step(1.0)
        .width(160.0),
        w::container(w::mouse_area(
            sweeten::widget::toggler(muted)
                .on_toggle(move |toggled| ContextMenuMessage::Participant {
                    member: toggler_member.clone(),
                    message: ParticipantMessage::Muted(toggled),
                })
                .size(structure.settings.checkbox_height),
        ))
        .padding(structure.small_gap),
    ]
    .spacing(structure.small_gap)
    .align_x(Alignment::Center)
    .boxed()
}
