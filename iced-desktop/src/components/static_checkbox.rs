use deplace_core::{structure::Structure, theme::Theme};
use iced::{Alignment, Border, Color, Element, Widget, widget as w};

use crate::modules::home::CHECKMARK_ICON;

use super::phosphor_icon;

pub fn static_checkbox<'a, T: 'a>(
    toggled: bool,
    color: impl Into<Color>,
    theme: Theme,
    structure: Structure,
    size: f32,
    hovered: bool,
) -> Element<'a, T> {
    let content = toggled.then(|| phosphor_icon(CHECKMARK_ICON, size * 0.8).color(theme.solid_bg));
    let color = color.into();

    w::container(content)
        .width(size)
        .height(size)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_| w::container::Style {
            background: toggled.then_some(color.into()),
            border: Border {
                color: if toggled {
                    color
                } else if hovered {
                    theme.text.normal.into()
                } else {
                    theme.border.into()
                },
                width: structure.border_thickness,
                radius: structure.smaller_border_radius.into(),
            },
            ..Default::default()
        })
        .boxed()
}
