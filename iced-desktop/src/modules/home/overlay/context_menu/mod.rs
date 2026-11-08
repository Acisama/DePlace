use deplace_core::room::call::CallMember;
use macros::iced_cache;
use matrix_sdk_ui::{Timeline, timeline::TimelineEventItemId};
use message::{MessageContextMenu, MessageMessage};
use participant::{ParticipantContextMenu, ParticipantMessage, percent_to_amplitude};

use crate::{common::*, modules::home::chat::MessageEvent};

use super::ModifyItem;

mod message;
mod participant;

#[derive(Clone, Debug)]
pub enum ContextMenuMessage {
    Message(MessageMessage),
    Participant(ParticipantMessage),
}

pub enum ContextMenuAction {
    SetReplyingTo {
        item_id: String,
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
    },
    OpenModifyItem(ModifyItem),
    Run(Task<()>),
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

#[derive(Clone, Debug, Hash, PartialEq)]
pub enum ContextMenuKind {
    Message(MessageContextMenu),
    /// One call participant: their playback volume and mute state.
    Participant(ParticipantContextMenu),
}

impl ContextMenuKind {
    pub fn call_participant(room: DePlaceRoom, member: CallMember, own_device: bool) -> Self {
        Self::Participant(ParticipantContextMenu::new(room, member, own_device))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn message(
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
        Self::Message(MessageContextMenu::new(
            timeline, event, room_id, identifier, item_id, is_pinned, can_edit, can_reply, can_pin,
            can_redact, copy_text, source,
        ))
    }
}

impl IcedWidget<ContextMenuMessage, ContextMenuAction> for ContextMenu {
    fn update(&mut self, message: ContextMenuMessage) -> Option<ContextMenuAction> {
        match message {
            ContextMenuMessage::Message(message) => {
                let ContextMenuKind::Message(MessageContextMenu {
                    timeline,
                    event,
                    room_id,
                    identifier,
                    item_id,
                    ..
                }) = &mut self.kind
                else {
                    return None;
                };
                match message {
                    MessageMessage::SetReplyingTo(event_id) => {
                        Some(ContextMenuAction::SetReplyingTo {
                            item_id: item_id.clone(),
                            room_id: room_id.clone(),
                            event_id,
                        })
                    }
                    MessageMessage::Delete => Some(ContextMenuAction::OpenModifyItem(
                        ModifyItem::delete(timeline.clone(), identifier.clone(), event.clone()),
                    )),
                    MessageMessage::Pin {
                        event_id,
                        is_pinned,
                    } => Some(ContextMenuAction::OpenModifyItem(ModifyItem::pin(
                        timeline.clone(),
                        identifier.clone(),
                        event_id,
                        event.clone(),
                        is_pinned,
                    ))),
                    // TODO
                    MessageMessage::Edit(_) => None,
                    MessageMessage::CopyText(text) => Some(ContextMenuAction::Run(
                        iced::clipboard::write((*text).clone()).map(|_| ()),
                    )),
                }
            }
            ContextMenuMessage::Participant(message) => {
                let ContextMenuKind::Participant(ParticipantContextMenu {
                    room,
                    member,
                    own_device,
                    muted_hovered,
                    volume_hovered,
                }) = &mut self.kind
                else {
                    return None;
                };

                match message {
                    // How loud they play for us, straight into the
                    // handle: the audio pump picks it up on the next
                    // frame, no message loop wake needed. Sliding to
                    // 0% mutes them, sliding anywhere above unmutes
                    // them again.
                    ParticipantMessage::Volume(percent) => {
                        let handle = member.handle();
                        handle.set_playback_volume(percent_to_amplitude(percent));
                        handle.set_playback_muted(percent <= 0.0);
                    }
                    // The playback volume stays where it is: unmuting
                    // returns to whatever they played at before muting
                    ParticipantMessage::Muted(muted) => {
                        if *own_device {
                            room.call().set_own_mic_muted(muted);
                        } else {
                            member.handle().set_playback_muted(muted);
                        }
                    }
                    ParticipantMessage::MutedHovered(hovered) => {
                        *muted_hovered = hovered;
                    }
                    ParticipantMessage::VolumeHovered(hovered) => {
                        *volume_hovered = hovered;
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
                ContextMenuKind::Message(menu) => menu
                    .view(theme, structure)
                    .map_el(ContextMenuMessage::Message),
                ContextMenuKind::Participant(menu) => menu
                    .view(theme, structure)
                    .map_el(ContextMenuMessage::Participant),
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
