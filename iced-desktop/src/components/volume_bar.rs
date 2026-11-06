use std::sync::Arc;

use deplace_core::call::VolumeCell;
use iced::widget::slider::{self, Slider};
use iced::{Border, Color, Element};

/// A read-only level meter for a live volume the audio thread keeps updating.
///
/// Rendered as an `iced` [`Slider`] with its handle hidden, so the rail alone
/// shows the level: full and green at maximum volume, emptier and redder the
/// quieter it gets.
pub fn volume_bar<'a, Message: 'a + Default + Clone>(
    volume: Arc<VolumeCell>,
    width: f32,
    height: f32,
    track: Color,
) -> Element<'a, Message> {
    let value = volume.get().clamp(0.0, 1.0);
    let fill = Color::from_rgb(1.0 - value, value, 0.0);

    Slider::new(0.0..=1.0, value, |_| Message::default())
        .width(width)
        .height(height)
        .style(move |_theme, _status| slider::Style {
            rail: slider::Rail {
                backgrounds: (fill.into(), track.into()),
                width: height,
                border: Border {
                    radius: (height / 2.0).into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 0.0 },
                background: Color::TRANSPARENT.into(),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        })
        .into()
}
