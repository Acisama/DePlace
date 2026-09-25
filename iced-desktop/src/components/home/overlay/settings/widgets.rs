use crate::common::*;
use deplace_core::settings::{EnumVariants, MatrixSettingField};
use enumset::{EnumSet, EnumSetType};
use iced::{Alignment, Length};
use phosphor_svgs::icon as icons;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::watch;

pub trait ToggleCloudExt {
    fn toggle_cloud(field_name: &'static str, uses_cloud: bool) -> Self;
}

pub trait SettingWidget:
    Sized + Clone + PartialEq + std::hash::Hash + Serialize + DeserializeOwned + Send + Sync + 'static
{
    fn render<Message: Clone + ToggleCloudExt + 'static + Default>(
        field: &MatrixSettingField<Self>,
        theme: Theme,
        structure: Structure,
        on_change: impl Fn(usize) -> Message + 'static,
    ) -> Element<'static, Message>;

    fn commit(field: &MatrixSettingField<Self>, idx: usize) -> Self;
}

/// Computes the new value for a field from a widget selection index and
/// spawns the task that persists it.
pub fn commit_task<T: SettingWidget>(
    field: &MatrixSettingField<T>,
    settings: &Settings,
    idx: usize,
) -> Task<()> {
    let field = field.clone();
    let settings = settings.clone();
    let val = T::commit(&field, idx);

    Task::future(async move {
        field.set(val, &settings).await;
    })
}

impl SettingWidget for bool {
    fn render<Message: Clone + ToggleCloudExt + 'static + Default>(
        field: &MatrixSettingField<bool>,
        theme: Theme,
        structure: Structure,
        on_change: impl Fn(usize) -> Message + 'static,
    ) -> Element<'static, Message> {
        let checked = field.value();

        let switch = w::mouse_area(
            sweeten::widget::toggler(checked)
                .on_toggle(move |_| on_change(0))
                .size(structure.settings.checkbox_height)
                .style(move |_, status| {
                    let is_toggled = match status {
                        sweeten::widget::toggler::Status::Active { is_toggled }
                        | sweeten::widget::toggler::Status::Hovered { is_toggled }
                        | sweeten::widget::toggler::Status::Disabled { is_toggled } => is_toggled,
                    };

                    sweeten::widget::toggler::Style {
                        background: if is_toggled {
                            theme.colors.success.into()
                        } else {
                            theme.text.muted.into()
                        },
                        background_border_width: structure.border_thickness,
                        background_border_color: theme.border.into(),
                        foreground: theme.solid_bg.into(),
                        foreground_border_width: 0.0,
                        foreground_border_color: Color::TRANSPARENT,
                        text_color: None,
                        border_radius: None,
                        padding_ratio: 0.1,
                    }
                }),
        )
        .interaction(Interaction::Pointer);

        setting_row(field, theme, structure, switch.into(), false)
    }

    fn commit(field: &MatrixSettingField<bool>, _idx: usize) -> bool {
        !field.value()
    }
}

fn render_dropdown<T, Message>(
    field: &MatrixSettingField<T>,
    theme: Theme,
    structure: Structure,
    on_change: impl Fn(usize) -> Message + 'static,
) -> Element<'static, Message>
where
    T: EnumVariants + Clone + PartialEq + Serialize + DeserializeOwned + Send + Sync + 'static,
    Message: Clone + ToggleCloudExt + 'static,
{
    let variants: Vec<(T, &'static str, usize)> = T::all_variants()
        .enumerate()
        .map(|(idx, (val, label))| (val, label, idx))
        .collect();
    let current = {
        let value = field.value();
        let (label, idx) = variants
            .iter()
            .find(|(v, _, _)| v == &value)
            .map(|(_, s, idx)| (*s, *idx))
            .unwrap_or(("", 0));
        (value, label, idx)
    };

    let dropdown = w::pick_list(Some(current), variants, move |(_, label, _)| {
        (*label).into()
    })
    .on_select(move |(_, _, idx)| on_change(idx))
    .width(structure.settings.dropdown_width)
    .text_size(structure.font_size)
    .style(move |_, status| w::pick_list::Style {
        text_color: theme.text.normal.into(),
        placeholder_color: theme.text.dim.into(),
        handle_color: theme.text.dim.into(),
        background: theme.solid_bg.into(),
        border: Border {
            color: if matches!(status, w::pick_list::Status::Opened { .. }) {
                theme.accent.into()
            } else {
                theme.border.into()
            },
            width: structure.border_thickness,
            radius: structure.semi_border_radius().into(),
        },
    })
    .menu_style(move |_| w::overlay::menu::Style {
        background: theme.solid_bg.into(),
        // disabled_background: theme.solid_bg.into(),
        border: Border {
            color: theme.border.into(),
            width: structure.border_thickness,
            radius: structure.semi_border_radius().into(),
        },
        text_color: theme.text.dim.into(),
        // disabled_text_color: theme.text.muted.into(),
        selected_text_color: theme.text.normal.into(),
        selected_background: theme.solid_hover_bg.into(),
        // label_text_color: theme.text.normal.into(),
        // separator_color: theme.border.into(),
        shadow: Default::default(),
    });

    setting_row(field, theme, structure, dropdown.into(), false)
}

fn commit_dropdown<T: EnumVariants>(idx: usize, fallback: T) -> T {
    T::all_variants()
        .nth(idx)
        .map(|(variant, _)| variant)
        .unwrap_or(fallback)
}

macro_rules! impl_dropdown_widget {
    ($($ty:ty),* $(,)?) => {
        $(
            impl SettingWidget for $ty {
                fn render<Message: Clone + ToggleCloudExt + 'static>(
                    field: &MatrixSettingField<$ty>,
                    theme: Theme,
                    structure: Structure,
                    on_change: impl Fn(usize) -> Message + 'static,
                ) -> Element<'static, Message> {
                    render_dropdown(field, theme, structure, on_change)
                }

                fn commit(field: &MatrixSettingField<$ty>, idx: usize) -> $ty {
                    commit_dropdown(idx, field.value())
                }
            }
        )*
    };
}

impl_dropdown_widget!(
    deplace_core::settings::HourFormat,
    deplace_core::settings::DateFormat,
    deplace_core::settings::DayOfWeek,
    deplace_core::settings::DataSizeUnit,
    chrono_tz::Tz,
);

impl<T> SettingWidget for EnumSet<T>
where
    T: EnumSetType + EnumVariants + 'static,
    EnumSet<T>: Clone + PartialEq + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn render<Message: Clone + ToggleCloudExt + 'static>(
        field: &MatrixSettingField<EnumSet<T>>,
        theme: Theme,
        structure: Structure,
        on_change: impl Fn(usize) -> Message + 'static,
    ) -> Element<'static, Message> {
        let mut variants: Vec<(usize, T, &'static str)> = T::all_variants()
            .enumerate()
            .map(|(idx, (variant, label))| (idx, variant, label))
            .collect();
        variants.sort_by_key(|(_, _, label)| *label);
        let active = field.value();

        let chips = variants.into_iter().map(|(idx, variant, label)| {
            let is_active = active.contains(variant);

            w::button(w::text(label).size(structure.font_size).center())
                .padding(structure.small_gap)
                .on_press(on_change(idx))
                .style(move |_, status| ButtonStyle {
                    text_color: if status.active() {
                        theme.text.normal.into()
                    } else if is_active {
                        theme.colors.success.into()
                    } else {
                        theme.text.dim.into()
                    },
                    background: is_active.then_some(
                        Color {
                            a: 0.1,
                            ..theme.colors.success.into()
                        }
                        .into(),
                    ),
                    border: Border {
                        color: if status.active() {
                            theme.accent.into()
                        } else if is_active {
                            theme.colors.success.into()
                        } else {
                            theme.border.into()
                        },
                        width: structure.border_thickness,
                        radius: structure.inner_border_radius.into(),
                    },
                    ..Default::default()
                })
                .into()
        });

        let grid = w::grid(chips)
            .spacing(structure.small_gap)
            .columns(5)
            .height(Length::Shrink);

        setting_row(field, theme, structure, grid.into(), true)
    }

    fn commit(field: &MatrixSettingField<EnumSet<T>>, idx: usize) -> EnumSet<T> {
        let mut current = field.value();
        if let Some((variant, _)) = T::all_variants().nth(idx) {
            if current.contains(variant) {
                current.remove(variant);
            } else {
                current.insert(variant);
            }
        }
        current
    }
}

fn cloud_button<Message: 'static + Clone + ToggleCloudExt>(
    field_name: &'static str,
    uses_cloud: &Option<watch::Sender<bool>>,
    theme: Theme,
    structure: Structure,
) -> Element<'static, Message> {
    let current = uses_cloud.as_ref().map(|b| *b.borrow());

    let (icon, color, hover_color, tooltip) = match current {
        Some(true) => (
            phosphor_svgs::icon::cloud::REGULAR,
            theme.accent,
            Some(theme.text.normal),
            "This setting is synced across all devices",
        ),
        Some(false) => (
            phosphor_svgs::icon::cloud_slash::REGULAR,
            theme.text.dim,
            Some(theme.text.normal),
            "This setting is not synced across all devices",
        ),
        None => (
            phosphor_svgs::icon::cloud_slash::REGULAR,
            theme.text.muted,
            Some(theme.text.muted),
            "This setting cannot be synced across all devices",
        ),
    };

    themed_tooltip(
        w::button(phosphor_icon(icon, structure.font_size * 1.2))
            .padding(0.0)
            .style(move |_, status| ButtonStyle {
                text_color: if let Some(hover_color) = hover_color
                    && status.active()
                {
                    hover_color
                } else {
                    color
                }
                .into(),
                ..Default::default()
            })
            .on_press_maybe(current.map(|is_synced| Message::toggle_cloud(field_name, !is_synced))),
        tooltip,
        structure,
        theme,
    )
    .into()
}

fn setting_row<T, Message: 'static + Clone + ToggleCloudExt>(
    field: &MatrixSettingField<T>,
    theme: Theme,
    structure: Structure,
    control: Element<'static, Message>,
    extra_row: bool,
) -> Element<'static, Message> {
    let cloud_button = cloud_button(field.local_name, &field.uses_cloud, theme, structure);
    let text = themed_tooltip(
        w::text(field.human_readable)
            .color(theme.text.normal)
            .size(structure.font_size),
        field.description,
        structure,
        theme,
    );
    let filler = w::space().width(Fill);

    if extra_row {
        w::column![
            w::row![text, filler, cloud_button]
                .align_y(Alignment::Center)
                .padding(structure.small_gap / 2.0)
                .spacing(structure.small_gap),
            control
        ]
        .spacing(structure.small_gap)
        .into()
    } else {
        w::row![text, filler, control, cloud_button]
            .align_y(Alignment::Center)
            .padding(structure.small_gap / 2.0)
            .spacing(structure.small_gap)
            .into()
    }
}

pub fn render_subsection<Message: Clone + 'static>(
    title: &'static str,
    expanded: bool,
    toggle: Message,
    theme: Theme,
    structure: Structure,
    content: Element<'static, Message>,
) -> Element<'static, Message> {
    let header = w::button(
        w::row![
            weighted_text(title, Weight::Bold)
                .size(structure.font_size * 1.1)
                .color(theme.text.normal),
            w::container("")
                .width(Fill)
                .height(structure.divider_width)
                .style(move |_| ContainerStyle {
                    background: Some(theme.border.into()),
                    border: border::rounded(structure.divider_width / 2.0),
                    ..Default::default()
                }),
            phosphor_icon(
                if expanded {
                    icons::caret_up::BOLD
                } else {
                    icons::caret_down::BOLD
                },
                structure.font_size * 1.2,
            ),
        ]
        .align_y(Alignment::Center)
        .spacing(structure.small_gap),
    )
    .on_press(toggle)
    .padding(structure.small_gap / 2.0)
    .width(Fill)
    .style(move |_, status| ButtonStyle {
        text_color: if status.active() {
            theme.text.normal.into()
        } else {
            theme.text.dim.into()
        },
        background: None,
        border: Border::default(),
        ..Default::default()
    });

    let mut column = w::column![header]
        .spacing(structure.small_gap)
        .padding(structure.small_gap)
        .width(Fill);
    if expanded {
        column = column.push(content);
    }

    column.into()
}

pub fn render_spacer<Message: Clone + 'static>(structure: Structure) -> Element<'static, Message> {
    w::space().height(structure.gap).into()
}
