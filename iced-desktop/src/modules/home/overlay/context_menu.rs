use deplace_core::room::call::CallMember;
use iced::{Alignment, Radians, gradient::Linear};
use macros::iced_cache;
use matrix_sdk_ui::{Timeline, timeline::TimelineEventItemId};

use crate::{
    common::*,
    components::{
        PushElement, color_stops, equal_width::equal_width, phosphor_icon,
        static_checkbox::static_checkbox,
    },
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
    MutedHovered(bool),
    VolumeHovered(bool),
}

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
    Participant {
        room: DePlaceRoom,

        member: CallMember,
        own_device: bool,
        muted_hovered: bool,
        volume_hovered: bool,
    },
}

impl ContextMenuKind {
    pub fn call_participant(room: DePlaceRoom, member: CallMember, own_device: bool) -> Self {
        Self::Participant {
            room,
            member,
            own_device,
            muted_hovered: false,
            volume_hovered: false,
        }
    }
}

impl Hash for ContextMenuKind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            ContextMenuKind::Message { .. } => 0.hash(state),
            // The member handle holds the live volume and mute state,
            // the menu reads them on every redraw instead of being
            // invalidated by them
            ContextMenuKind::Participant {
                member,
                muted_hovered,
                volume_hovered,
                ..
            } => {
                1.hash(state);
                member.member.user_id().hash(state);
                member.device_id.hash(state);
                muted_hovered.hash(state);
                volume_hovered.hash(state);
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
                ContextMenuKind::Participant {
                    member: member1, ..
                },
                ContextMenuKind::Participant {
                    member: member2, ..
                },
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
            ContextMenuMessage::Message(message) => {
                let ContextMenuKind::Message {
                    timeline,
                    event,
                    room_id,
                    identifier,
                    item_id,
                    ..
                } = &mut self.kind
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
                }
            }
            ContextMenuMessage::Participant(message) => {
                let ContextMenuKind::Participant {
                    room,
                    member,
                    own_device,
                    muted_hovered,
                    volume_hovered,
                } = &mut self.kind
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
                ContextMenuKind::Message {
                    is_pinned,

                    can_edit,
                    can_reply,
                    can_pin,
                    can_redact,
                    ..
                } => render_message_context_menu(
                    theme,
                    structure,
                    *is_pinned,
                    can_edit.clone(),
                    can_reply.clone(),
                    can_pin.clone(),
                    *can_redact,
                ),
                ContextMenuKind::Participant {
                    member,
                    own_device,
                    muted_hovered,
                    volume_hovered,
                    ..
                } => render_participant_context_menu(
                    theme,
                    structure,
                    member.clone(),
                    *own_device,
                    *muted_hovered,
                    *volume_hovered,
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
        .boxed()
    }
}

#[allow(clippy::too_many_arguments)]
fn render_message_context_menu(
    theme: Theme,
    structure: Structure,
    is_pinned: bool,
    can_edit: Option<OwnedEventId>,
    can_reply: Option<OwnedEventId>,
    can_pin: Option<OwnedEventId>,
    can_redact: bool,
) -> Element<'static, ContextMenuMessage> {
    let mut buttons = Vec::new();

    let message = move |message| ContextMenuMessage::Message(message);

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

/// Loudness is perceived on a log scale: roughly every +10dB sounds
/// twice as loud, which in amplitude terms is a ~3.16x jump, not 2x.
/// This is the exponent that turns the volume slider's linear percentage
/// into an amplitude multiplier that matches that perception, so that
/// e.g. 200% actually sounds twice as loud and 400% four times as loud,
/// derived from `amplitude = (percent / 100).powf(log2(10) / 2)`.
const LOUDNESS_EXPONENT: f32 = 1.660_964;

/// Curves a slider percentage (0..=400) into the amplitude multiplier
/// the audio pump should scale samples by, see [`LOUDNESS_EXPONENT`].
fn percent_to_amplitude(percent: f32) -> f32 {
    (percent / 100.0).powf(LOUDNESS_EXPONENT)
}

/// Inverse of [`percent_to_amplitude`], for drawing the slider at the
/// position matching a stored amplitude multiplier.
fn amplitude_to_percent(amplitude: f32) -> f32 {
    100.0 * amplitude.powf(1.0 / LOUDNESS_EXPONENT)
}

/// The menu for one call participant
///
///  A slider for their playback volume and a mute toggle, maybe
/// should be a togglebox
///
/// Both live values live in the member's call handle, the menu reads
/// them fresh on every redraw so slider and toggle always agree.
fn render_participant_context_menu(
    theme: Theme,
    structure: Structure,
    member: CallMember,
    own_device: bool,
    muted_hovered: bool,
    volume_hovered: bool,
) -> Element<'static, ContextMenuMessage> {
    let handle = member.handle();
    let volume = handle.playback_volume();
    let percent = amplitude_to_percent(volume).round();
    // Our own mute state lives in the participant's `muted` flag (driven
    // by the mic track itself), not `playback_muted`, which only tracks
    // whether *we* silence someone else's playback.
    let muted = if own_device {
        member.is_muted()
    } else {
        handle.playback_muted()
    };

    let message = move |message| ContextMenuMessage::Participant(message);

    let mut column = w::Column::new();

    let max_volume = 400.0;
    let threshold = 100.0;

    // TODO: Find a good way to calculate that
    let width = 200.0;

    let handle_width = structure.font_size;
    let track_width = width - structure.small_gap * 4.0;
    let track_x =
        |value: f32| (track_width - handle_width) * value / max_volume + handle_width / 2.0;

    let slider_bg_color = if volume_hovered {
        theme.text.dim
    } else {
        theme.solid_hover_bg
    };

    let (before_bg, after_bg) = if percent <= threshold {
        let threshold_point =
            (track_x(threshold) - track_x(percent)) / (track_width - track_x(percent));
        (
            theme.accent.into(),
            Linear::new(Radians::PI / 2.0)
                .add_stops(color_stops([
                    (0.0, slider_bg_color),
                    (threshold_point - 0.001, slider_bg_color),
                    (threshold_point, theme.error_half_blended().scale_alpha(0.5)),
                    (1.0, theme.error_half_blended().scale_alpha(0.5)),
                ]))
                .into(),
        )
    } else {
        let threshold_point = track_x(threshold) / track_x(percent);
        (
            Linear::new(Radians::PI / 2.0)
                .add_stops(color_stops([
                    (0.0, theme.accent),
                    (threshold_point - 0.001, theme.accent),
                    (threshold_point, theme.colors.error),
                    (1.0, theme.colors.error),
                ]))
                .into(),
            theme.error_half_blended().scale_alpha(0.5).into(),
        )
    };

    if !own_device {
        column = column.push_element(
            w::container(
                w::column![
                    w::text(format!("Volume: {}%", percent as u32))
                        .color(if volume_hovered {
                            theme.text.normal
                        } else {
                            theme.text.dim
                        })
                        .size(structure.font_size),
                    w::mouse_area(
                        w::slider(0.0..=400.0, percent, move |value| {
                            message(ParticipantMessage::Volume(value))
                        })
                        .step(10.0)
                        .shift_step(1.0)
                        .style(move |_, _| w::slider::Style {
                            rail: w::slider::Rail {
                                backgrounds: (before_bg, after_bg),
                                width: structure.font_size / 2.0,
                                border: border::rounded(structure.font_size / 4.0),
                            },
                            handle: w::slider::Handle {
                                shape: w::slider::HandleShape::Circle {
                                    radius: structure.font_size / 2.0,
                                },
                                background: if percent <= threshold {
                                    theme.accent.into()
                                } else {
                                    theme.colors.error.into()
                                },
                                border_color: Color::TRANSPARENT,
                                border_width: 0.0,
                            },
                        }),
                    )
                    .on_enter(message(ParticipantMessage::VolumeHovered(true)))
                    .on_exit(message(ParticipantMessage::VolumeHovered(false))),
                ]
                .spacing(structure.small_gap),
            )
            .maybe_colored_rounded(
                volume_hovered.then_some(theme.solid_hover_bg),
                structure.semi_large_border_radius(),
            )
            .padding(structure.small_gap),
        );
    }

    column = column.push_element(
        w::mouse_area(
            w::container(
                w::row![
                    w::text("Mute")
                        .size(structure.font_size)
                        .color(if muted_hovered {
                            theme.text.normal
                        } else {
                            theme.text.dim
                        }),
                    Space::new().width(Fill),
                    static_checkbox(
                        muted,
                        theme.colors.error,
                        theme,
                        structure,
                        structure.font_size * 1.1,
                        muted_hovered
                    )
                ]
                .width(Fill)
                .align_y(Alignment::Center),
            )
            .maybe_colored_rounded(
                muted_hovered.then_some(theme.solid_hover_bg),
                structure.semi_large_border_radius(),
            )
            .padding(structure.small_gap),
        )
        .interaction(Interaction::Pointer)
        .on_enter(message(ParticipantMessage::MutedHovered(true)))
        .on_exit(message(ParticipantMessage::MutedHovered(false)))
        .on_press(message(ParticipantMessage::Muted(!muted))),
    );

    column
        .spacing(structure.small_gap)
        .padding(structure.small_gap)
        .width(width)
        .boxed()
}
