use deplace_core::{structure::Structure, theme::Theme};
use iced::{
    Border, Element, padding,
    widget::{self as w, text::IntoFragment},
};

pub fn themed_tooltip_text<'a, T: 'a>(
    content: impl Into<Element<'a, T>>,
    tooltip: impl IntoFragment<'a>,
    structure: Structure,
    theme: Theme,
) -> w::tooltip::Tooltip<'a, T> {
    themed_tooltip_content(
        content,
        w::container(w::text(tooltip).color(theme.text.normal)),
        structure,
        theme,
    )
}

pub fn themed_tooltip_content<'a, T: 'a>(
    content: impl Into<Element<'a, T>>,
    tooltip_content: impl Into<Element<'a, T>>,
    structure: Structure,
    theme: Theme,
) -> w::tooltip::Tooltip<'a, T> {
    w::tooltip(
        content,
        w::container(tooltip_content)
            .padding(padding::horizontal(structure.small_gap).vertical(structure.small_gap / 2.0)),
        w::tooltip::Position::Bottom,
    )
    .delay(std::time::Duration::from_millis(300))
    .style(move |_| w::container::Style {
        background: Some(theme.solid_bg.into()),
        border: Border {
            color: theme.border.into(),
            width: structure.border_thickness,
            radius: structure.inner_border_radius.into(),
        },
        text_color: Some(theme.text.normal.into()),
        ..Default::default()
    })
}
