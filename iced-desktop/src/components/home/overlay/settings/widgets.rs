use crate::common::*;
use deplace_core::settings::{EnumVariants, MatrixSettingField};
use enumset::{EnumSet, EnumSetType};
use iced::Alignment;
use phosphor_svgs::icon as icons;
use serde::{Serialize, de::DeserializeOwned};

pub trait SettingWidget:
    Sized + Clone + PartialEq + std::hash::Hash + Serialize + DeserializeOwned + Send + Sync + 'static
{
    fn render<Message: Clone + 'static>(
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
    fn render<Message: Clone + 'static>(
        field: &MatrixSettingField<bool>,
        theme: Theme,
        structure: Structure,
        on_change: impl Fn(usize) -> Message + 'static,
    ) -> Element<'static, Message> {
        let checked = field.value();

        let switch = w::toggler(checked)
            .on_toggle(move |_| on_change(0))
            .size(structure.settings.checkbox_height)
            .style(move |_, status| {
                let is_toggled = match status {
                    w::toggler::Status::Active { is_toggled }
                    | w::toggler::Status::Hovered { is_toggled }
                    | w::toggler::Status::Disabled { is_toggled } => is_toggled,
                };

                w::toggler::Style {
                    background: if is_toggled {
                        theme.colors.success.into()
                    } else {
                        theme.text.muted.into()
                    },
                    background_border_width: structure.border_thickness,
                    background_border_color: theme.border,
                    foreground: theme.solid_bg.into(),
                    foreground_border_width: 0.0,
                    foreground_border_color: Color::TRANSPARENT,
                    text_color: None,
                    border_radius: None,
                    padding_ratio: 0.1,
                }
            });

        setting_row(field, theme, structure, switch.into())
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
    Message: Clone + 'static,
{
    let variants: Vec<(T, &'static str)> = T::all_variants().collect();
    let current = field.value();
    let options: Vec<T> = variants.iter().map(|(v, _)| v.clone()).collect();

    let labels_for_display = variants.clone();
    let labels_for_select = variants;

    let dropdown = w::pick_list(Some(current), options, move |value: &T| {
        labels_for_display
            .iter()
            .find(|(v, _)| v == value)
            .map(|(_, label)| label.to_string())
            .unwrap_or_default()
    })
    .on_select(move |value: T| {
        let idx = labels_for_select
            .iter()
            .position(|(v, _)| v == &value)
            .unwrap_or(0);
        on_change(idx)
    })
    .width(structure.settings.dropdown_width)
    .text_size(structure.font_size)
    .style(move |_, status| w::pick_list::Style {
        text_color: theme.text.normal,
        placeholder_color: theme.text.dim,
        handle_color: theme.text.dim,
        background: theme.solid_bg.into(),
        border: Border {
            color: if matches!(status, w::pick_list::Status::Opened { .. }) {
                theme.accent
            } else {
                theme.border
            },
            width: structure.border_thickness,
            radius: structure.semi_border_radius().into(),
        },
    });

    setting_row(field, theme, structure, dropdown.into())
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
                fn render<Message: Clone + 'static>(
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
    fn render<Message: Clone + 'static>(
        field: &MatrixSettingField<EnumSet<T>>,
        theme: Theme,
        structure: Structure,
        on_change: impl Fn(usize) -> Message + 'static,
    ) -> Element<'static, Message> {
        let mut variants: Vec<(T, &'static str)> = T::all_variants().collect();
        variants.sort_by_key(|(_, label)| *label);
        let active = field.value();

        let chips = variants
            .into_iter()
            .enumerate()
            .map(|(idx, (variant, label))| {
                let is_active = active.contains(variant);

                w::button(w::text(label).size(structure.font_size * 0.9))
                    .padding(structure.small_gap / 2.0)
                    .on_press(on_change(idx))
                    .style(move |_, _| ButtonStyle {
                        text_color: if is_active {
                            theme.colors.success
                        } else {
                            theme.text.dim
                        },
                        background: is_active.then_some(
                            Color {
                                a: 0.1,
                                ..theme.colors.success
                            }
                            .into(),
                        ),
                        border: Border {
                            color: if is_active {
                                theme.colors.success
                            } else {
                                theme.border
                            },
                            width: structure.border_thickness,
                            radius: structure.inner_border_radius.into(),
                        },
                        ..Default::default()
                    })
                    .into()
            });

        let grid = w::row(chips).spacing(structure.small_gap).wrap();

        setting_row(field, theme, structure, grid.into())
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

fn setting_row<T, Message: 'static>(
    field: &MatrixSettingField<T>,
    theme: Theme,
    structure: Structure,
    control: Element<'static, Message>,
) -> Element<'static, Message> {
    w::row![
        w::text(field.human_readable)
            .color(theme.text.normal)
            .size(structure.font_size),
        w::space(),
        control,
    ]
    .align_y(Alignment::Center)
    .padding(structure.small_gap / 2.0)
    .into()
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
            theme.text.normal
        } else {
            theme.text.dim
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
