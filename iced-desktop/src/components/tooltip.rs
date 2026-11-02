use deplace_core::{structure::Structure, theme::Theme};
use iced::{
    Border, Pixels, padding,
    widget::{self as w, Container, Text, text::IntoFragment},
};

pub fn themed_tooltip_text<'a, W>(
    content: W,
    tooltip: impl IntoFragment<'a>,
    structure: Structure,
    theme: Theme,
) -> w::tooltip::Tooltip<'a, W, Container<'a, Container<'a, Text<'a>>>> {
    themed_tooltip_content(
        content,
        w::container(w::text(tooltip).color(theme.text.normal)),
        structure,
        theme,
        w::tooltip::Position::Bottom,
        0,
    )
}

pub fn themed_tooltip_content<'a, W, V>(
    content: W,
    tooltip_content: V,
    structure: Structure,
    theme: Theme,
    position: w::tooltip::Position,
    gap: impl Into<Pixels>,
) -> w::tooltip::Tooltip<'a, W, Container<'a, V>> {
    w::tooltip(
        content,
        w::container(tooltip_content)
            .padding(padding::horizontal(structure.small_gap).vertical(structure.small_gap / 2.0)),
        position,
    )
    .gap(gap)
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
