use deplace_core::room::call::CallMember;
use iced::{Alignment, Radians, gradient::Linear};

use crate::{
    common::*,
    components::{PushElement, color_stops, static_checkbox::static_checkbox},
};

/// Loudness is perceived on a log scale: roughly every +10dB sounds
/// twice as loud, which in amplitude terms is a ~3.16x jump, not 2x.
/// This is the exponent that turns the volume slider's linear percentage
/// into an amplitude multiplier that matches that perception, so that
/// e.g. 200% actually sounds twice as loud and 400% four times as loud,
/// derived from `amplitude = (percent / 100).powf(log2(10) / 2)`.
const LOUDNESS_EXPONENT: f32 = 1.660_964;

/// Curves a slider percentage (0..=400) into the amplitude multiplier
/// the audio pump should scale samples by, see [`LOUDNESS_EXPONENT`].
pub fn percent_to_amplitude(percent: f32) -> f32 {
    (percent / 100.0).powf(LOUDNESS_EXPONENT)
}

/// Inverse of [`percent_to_amplitude`], for drawing the slider at the
/// position matching a stored amplitude multiplier.
fn amplitude_to_percent(amplitude: f32) -> f32 {
    100.0 * amplitude.powf(1.0 / LOUDNESS_EXPONENT)
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

#[derive(Debug, Clone)]
pub struct ParticipantContextMenu {
    pub room: DePlaceRoom,

    pub member: CallMember,
    pub own_device: bool,
    pub muted_hovered: bool,
    pub volume_hovered: bool,
}

impl PartialEq for ParticipantContextMenu {
    fn eq(&self, other: &Self) -> bool {
        self.member.member.user_id() == other.member.member.user_id()
            && self.member.device_id == other.member.device_id
    }
}

impl Hash for ParticipantContextMenu {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.member.member.user_id().hash(state);
        self.member.device_id.hash(state);
        self.muted_hovered.hash(state);
        self.volume_hovered.hash(state);
    }
}

impl ParticipantContextMenu {
    pub fn new(room: DePlaceRoom, member: CallMember, own_device: bool) -> Self {
        Self {
            room,
            member,
            own_device,
            muted_hovered: false,
            volume_hovered: false,
        }
    }

    pub fn view(&self, theme: Theme, structure: Structure) -> Element<'static, ParticipantMessage> {
        let handle = self.member.handle();
        let volume = handle.playback_volume();
        let percent = amplitude_to_percent(volume).round();
        // Our own mute state lives in the participant's `muted` flag (driven
        // by the mic track itself), not `playback_muted`, which only tracks
        // whether *we* silence someone else's playback.
        let muted = if self.own_device {
            self.member.is_muted()
        } else {
            handle.playback_muted()
        };

        let mut column = w::Column::new();

        let max_volume = 400.0;
        let threshold = 100.0;

        // TODO: Find a good way to calculate that
        let width = 200.0;

        let handle_width = structure.font_size;
        let track_width = width - structure.small_gap * 4.0;
        let track_x =
            |value: f32| (track_width - handle_width) * value / max_volume + handle_width / 2.0;

        let slider_bg_color = if self.volume_hovered {
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

        if !self.own_device {
            column = column.push_element(
                w::container(
                    w::column![
                        w::text(format!("Volume: {}%", percent as u32))
                            .color(if self.volume_hovered {
                                theme.text.normal
                            } else {
                                theme.text.dim
                            })
                            .size(structure.font_size),
                        w::mouse_area(
                            w::slider(0.0..=400.0, percent, move |value| {
                                ParticipantMessage::Volume(value)
                            })
                            .step(10.0)
                            .shift_step(1.0)
                            .style(move |_, _| {
                                w::slider::Style {
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
                                }
                            }),
                        )
                        .on_enter(ParticipantMessage::VolumeHovered(true))
                        .on_exit(ParticipantMessage::VolumeHovered(false)),
                    ]
                    .spacing(structure.small_gap),
                )
                .maybe_colored_rounded(
                    self.volume_hovered.then_some(theme.solid_hover_bg),
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
                            .color(if self.muted_hovered {
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
                            self.muted_hovered
                        )
                    ]
                    .width(Fill)
                    .align_y(Alignment::Center),
                )
                .maybe_colored_rounded(
                    self.muted_hovered.then_some(theme.solid_hover_bg),
                    structure.semi_large_border_radius(),
                )
                .padding(structure.small_gap),
            )
            .interaction(Interaction::Pointer)
            .on_enter(ParticipantMessage::MutedHovered(true))
            .on_exit(ParticipantMessage::MutedHovered(false))
            .on_press(ParticipantMessage::Muted(!muted)),
        );

        column
            .spacing(structure.small_gap)
            .padding(structure.small_gap)
            .width(width)
            .boxed()
    }
}
