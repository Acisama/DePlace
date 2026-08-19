use core::str;
use std::{cell::Cell, collections::HashSet, rc::Rc, sync::Arc, time::Duration};

use enumset::{EnumSet, EnumSetType};
use gpui::{
    Animation, AnimationExt, Bounds, Context, Deferred, Div, ElementId, Hsla, InteractiveElement,
    IntoElement, MouseDownEvent, ParentElement, Pixels, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, canvas, deferred, div, prelude::FluentBuilder, px,
    relative, transparent_black,
};
use gpui_component::StyledExt;
use macros::tailwind_div;
use serde::{Serialize, de::DeserializeOwned};
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{CustomStyles, TooltipExt, profiles::render_icon},
    things::{AppTheme, Structure},
};
use deplace_core::settings::{EnumVariants, MatrixSettingField, Settings};

use super::SettingsView;

pub(super) mod chats;
pub(super) mod general;

fn get_cloud_stuff(
    uses_cloud: &Option<watch::Sender<bool>>,
    theme: &AppTheme,
) -> (&'static str, Hsla, &'static str) {
    match uses_cloud.as_ref().map(|b| *b.borrow()) {
        Some(true) => (
            phosphor_svgs::icon::cloud::REGULAR,
            theme.accent,
            "This setting is synced across all devices",
        ),
        Some(false) => (
            phosphor_svgs::icon::cloud_slash::REGULAR,
            theme.text.dim,
            "This setting is not synced across all devices",
        ),
        None => (
            phosphor_svgs::icon::cloud_slash::REGULAR,
            theme.text.muted,
            "This setting can not be synced across all devices",
        ),
    }
}

fn cloud_button<T: Clone + Serialize + DeserializeOwned>(
    field: &MatrixSettingField<T>,
    settings: &Settings,
    theme: &AppTheme,
    structure: &Structure,
) -> Stateful<Div> {
    let (cloud_icon, cloud_color, cloud_tooltip) = get_cloud_stuff(&field.uses_cloud, theme);

    div()
        .text_color(cloud_color)
        .id("cloud-tooltip")
        .child(render_icon(cloud_icon, structure.font_size * 1.2))
        .custom_tooltip(cloud_tooltip, theme, structure)
        .when_some(
            field.uses_cloud.clone().map(|v| *v.borrow()),
            |el, uses_cloud| {
                el.on_click({
                    let field = field.clone();
                    let settings = settings.clone();
                    move |_, _, cx| {
                        cx.stop_propagation();
                        field.set_uses_cloud(!uses_cloud, &settings);
                    }
                })
            },
        )
}

fn name_div<T>(field: &MatrixSettingField<T>, theme: &AppTheme, structure: &Structure) -> Div {
    tailwind_div!(
        flex,
        items_center,
        text_center,
        flex_row,
        gap(structure.small_gap / 2.0),
        line_height(relative(1.0)),
    )
    .child(field.human_readable)
    .child(
        div()
            .id("name-tooltip")
            .child(render_icon(
                phosphor_svgs::icon::question::REGULAR,
                structure.font_size * 0.8,
            ))
            .custom_tooltip(field.description, theme, structure),
    )
}

fn setting_toggle(
    settings: &Settings,
    field: &MatrixSettingField<bool>,
    theme: &AppTheme,
    structure: &Structure,
    tokio_rt: Arc<Runtime>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> Stateful<Div> {
    let checked = field.value();

    let prev_checked = window.use_keyed_state(field.local_name, cx, |_, _| checked);

    tailwind_div!(
        flex,
        flex_grow_1,
        px(structure.small_gap / 2.0),
        py(structure.small_gap),
        justify_between,
        cursor_pointer,
        border_transparent,
        text_color(theme.text.dim),
        hover(
            border_color(theme.tile.border),
            text_color(theme.text.normal)
        ),
        rounded(structure.semi_border_radius()),
        items_center,
    )
    .id(field.local_name)
    .on_click({
        let field = field.clone();
        let settings = settings.clone();
        let tokio_rt = tokio_rt.clone();

        move |_, _, _| {
            let field = field.clone();
            let settings = settings.clone();
            let new_checked = !checked;
            tokio_rt.spawn(async move {
                field.set(new_checked, &settings).await;
            });
        }
    })
    .child(name_div(field, theme, structure))
    .child(
        tailwind_div!(flex, items_center, gap(structure.gap))
            .child({
                let width = structure.settings.checkbox_width;
                let height = structure.settings.checkbox_height;
                let tick_size = height;
                let max_x = width - tick_size;

                let bg = if checked {
                    theme.text.muted
                } else {
                    transparent_black()
                };
                let tick_color = if checked {
                    theme.colors.success
                } else {
                    theme.colors.error
                };

                div()
                    .id("toggle")
                    .focusable()
                    .w(width)
                    .h(height)
                    .rounded(height)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .border_1()
                    .border_color(theme.tile.border)
                    .focus(|style| style.border_color(theme.accent))
                    .bg(bg)
                    .child({
                        let thumb = div().rounded_full().bg(tick_color).size(tick_size);

                        if *prev_checked.read(cx) == checked {
                            let x = if checked { max_x } else { px(0.0) };
                            thumb.left(x).into_any_element()
                        } else {
                            let duration = Duration::from_secs_f64(0.15);

                            cx.spawn({
                                let prev_checked = prev_checked.clone();
                                async move |_, cx| {
                                    cx.background_executor().timer(duration).await;
                                    prev_checked.update(cx, |this, _| *this = checked);
                                }
                            })
                            .detach();

                            thumb
                                .with_animation(
                                    ElementId::NamedInteger("toggle-move".into(), checked as u64),
                                    Animation::new(duration),
                                    move |this, delta| {
                                        let x = if checked {
                                            max_x * delta
                                        } else {
                                            max_x - max_x * delta
                                        };
                                        this.left(x)
                                    },
                                )
                                .into_any_element()
                        }
                    })
                    .on_click({
                        let field = field.clone();
                        let settings = settings.clone();

                        move |_, _, _| {
                            let field = field.clone();
                            let settings = settings.clone();
                            let new_checked = !checked;
                            tokio_rt.spawn(async move {
                                field.set(new_checked, &settings).await;
                            });
                        }
                    })
            })
            .child(cloud_button(field, settings, theme, structure)),
    )
}

fn subsection(
    title: SharedString,
    id: SharedString,
    theme: &AppTheme,
    structure: &Structure,
    expanded_subsections: &HashSet<SharedString>,
    cx: &mut Context<SettingsView>,
) -> Div {
    let expanded = expanded_subsections.contains(&id);

    tailwind_div!(flex, flex_col, w_full, mb(structure.gap)).child(
        tailwind_div!(
            w_full,
            flex,
            flex_row,
            items_center,
            justify_between,
            cursor_pointer,
            p(structure.small_gap / 2.0),
            gap(structure.divider_width),
            border_transparent,
            text_color(theme.text.dim),
            hover(text_color(theme.text.normal))
        )
        .child(
            tailwind_div!(
                font_semibold,
                text_color(theme.text.normal),
                text_size(structure.font_size * 1.1),
                line_height(relative(1.0))
            )
            .child(title.clone()),
        )
        .child(tailwind_div!(
            flex,
            flex_grow_1,
            h(structure.divider_width),
            bg(theme.tile.border),
            mx(structure.gap)
        ))
        .child(
            tailwind_div!(flex, items_center, justify_center, cursor_pointer).child(render_icon(
                if expanded {
                    phosphor_svgs::icon::caret_up::REGULAR
                } else {
                    phosphor_svgs::icon::caret_down::REGULAR
                },
                structure.font_size * 1.2,
            )),
        )
        .id(id.clone())
        .on_click(cx.listener({
            let id = id.clone();
            move |view, _, _, cx| {
                if !view.expanded_subsections.remove(&id) {
                    view.expanded_subsections.insert(id.clone());
                }
                cx.notify();
            }
        })),
    )
}

fn spacer(structure: &Structure) -> Div {
    tailwind_div!(h(structure.gap * 2.0))
}

fn setting_dropdown<T>(
    settings: &Settings,
    field: &MatrixSettingField<T>,
    theme: &AppTheme,
    structure: &Structure,
    tokio_rt: Arc<Runtime>,
    active_dropdown: &Option<&'static str>,
    cx: &mut Context<SettingsView>,
) -> Stateful<Div>
where
    T: EnumVariants + Clone + PartialEq + Send + Sync + 'static,
{
    let options: Vec<_> = T::all_variants().collect();
    let current_val = field.value();

    let current_label = options
        .iter()
        .find(|(variant, _)| variant == &current_val)
        .map(|(_, label)| *label)
        .unwrap_or("Select...");

    let field_id = field.local_name;

    let is_open = active_dropdown == &Some(field_id);

    let popup_bounds: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));

    tailwind_div!(
        flex,
        flex_grow_1,
        p(structure.small_gap / 2.0),
        justify_between,
        cursor_pointer,
        border_transparent,
        text_color(theme.text.dim),
        hover(
            border_color(theme.tile.border),
            text_color(theme.text.normal)
        ),
        rounded(structure.semi_border_radius()),
        items_center,
    )
    .id(field_id)
    .child(name_div(field, theme, structure))
    .child(
        tailwind_div!(flex, items_center, gap(structure.gap))
            .child(
                tailwind_div!(
                    relative,
                    flex,
                    items_center,
                    justify_between,
                    gap(structure.small_gap / 2.0),
                    py(structure.small_gap / 2.0),
                    px(structure.small_gap),
                    rounded(structure.semi_border_radius()),
                    border_1,
                    border_color(theme.tile.border),
                    text_color(theme.text.normal),
                    cursor_pointer,
                    hover(border_color(theme.accent)),
                    w(structure.settings.dropdown_width),
                    bg(theme.solid_bg)
                )
                .id("dropdown")
                .on_click(cx.listener({
                    let id = field_id;
                    move |view, _, _, cx| {
                        if view.active_dropdown == Some(id) {
                            view.active_dropdown = None;
                        } else {
                            view.active_dropdown = Some(id);
                        }
                        cx.notify();
                    }
                }))
                .on_mouse_down_out({
                    let id = field_id;
                    let popup_bounds = popup_bounds.clone();
                    cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                        if view.active_dropdown == Some(id)
                            && !popup_bounds.get().contains(&event.position)
                        {
                            view.active_dropdown = None;
                            cx.notify();
                        }
                    })
                })
                .child(current_label)
                .child(tailwind_div!(text_color(theme.text.dim)).child(render_icon(
                    if is_open {
                        phosphor_svgs::icon::caret_up::REGULAR
                    } else {
                        phosphor_svgs::icon::caret_down::REGULAR
                    },
                    structure.font_size * 0.8,
                )))
                .children(is_open.then(|| {
                    dropdown_popup(
                        options,
                        current_val,
                        field,
                        settings,
                        theme,
                        structure,
                        tokio_rt,
                        popup_bounds,
                        cx,
                    )
                })),
            )
            .child(cloud_button(field, settings, theme, structure)),
    )
}

#[allow(clippy::too_many_arguments)]
fn dropdown_popup<T>(
    options: Vec<(T, &'static str)>,
    current_val: T,
    field: &MatrixSettingField<T>,
    settings: &Settings,
    theme: &AppTheme,
    structure: &Structure,
    tokio_rt: Arc<Runtime>,
    popup_bounds: Rc<Cell<Bounds<Pixels>>>,
    cx: &mut Context<SettingsView>,
) -> Deferred
where
    T: EnumVariants + Clone + PartialEq + Send + Sync + 'static,
{
    deferred(
        tailwind_div!(
            id("popup"),
            absolute,
            top(relative(1.0)),
            mt(structure.small_gap),
            left(px(-1.0)),
            flex,
            flex_col,
            w(structure.settings.dropdown_width),
            overflow_y_scroll,
            paddings(structure.small_gap / 2.0)
            border_1,
            flex,
            flex_col,
            gap(structure.divider_width),
            border_color(theme.tile.border),
            bg(theme.solid_bg),
            rounded(structure.semi_border_radius()),
        )
        .occlude()
        .child({
            let popup_bounds = popup_bounds.clone();
            canvas(
                move |bounds, _, _| popup_bounds.set(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        })
        .children(
            options
                .into_iter()
                .enumerate()
                .map(|(idx, (variant, label))| {
                    let is_selected = variant == current_val;
                    let field = field.clone();
                    let settings = settings.clone();
                    let tokio_rt = tokio_rt.clone();

                    tailwind_div!(
                        flex,
                        items_center,
                        justify_between,
                        rounded(structure.semi_border_radius()),
                        cursor_pointer,
                        text_color(theme.text.dim),
                        paddings(structure.small_gap / 2.0),
                        hover(bg(theme.solid_hover_bg))
                    )
                    .id(idx)
                    .when(!is_selected, |el| {
                        el.on_click(cx.listener(move |this, _, _, cx| {
                            let field = field.clone();
                            let settings = settings.clone();
                            let variant = variant.clone();
                            tokio_rt.spawn(async move {
                                field.set(variant, &settings).await;
                            });

                            this.active_dropdown = None;
                            cx.notify();
                        }))
                    })
                    .child(label)
                    .when(is_selected, |el| {
                        el.text_color(theme.text.normal)
                            .bg(theme.solid_hover_bg)
                            .cursor_default()
                            .child(tailwind_div!(text_color(theme.accent)).child(render_icon(
                                phosphor_svgs::icon::check::REGULAR,
                                structure.font_size * 0.8,
                            )))
                    })
                }),
        ),
    )
}

fn setting_enumset_toggles<T: EnumVariants + EnumSetType>(
    settings: &Settings,
    field: &MatrixSettingField<EnumSet<T>>,
    modes: &[(&'static str, &'static str, EnumSet<T>)],
    theme: &AppTheme,
    structure: &Structure,
    tokio_rt: Arc<Runtime>,
    num_cols: u16,
) -> Stateful<Div> {
    let mut variants: Vec<(T, &str)> = T::all_variants().collect();
    variants.sort_by_key(|(_, label)| *label);

    let active_variants = field.value();

    tailwind_div!(
        flex,
        flex_col,
        gap(structure.small_gap),
        rounded(structure.semi_border_radius()),
        px(structure.small_gap / 2.0),
        py(structure.small_gap / 1.5),
        border_transparent,
        text_color(theme.text.dim),
        hover(
            border_color(theme.tile.border),
            text_color(theme.text.normal)
        ),
    )
    .child(
        tailwind_div!(flex, flex_row, gap(structure.small_gap))
            .child(name_div(field, theme, structure))
            .when(!modes.is_empty(), |el| {
                el.child(tailwind_div!(flex, flex_wrap, gap(structure.small_gap)))
                    .children(modes.iter().map(|(label, description, mode_variants)| {
                        let is_active = &active_variants == mode_variants;

                        tailwind_div!(
                            py(structure.small_gap / 4.0),
                            px(structure.small_gap / 2.0),
                            rounded(structure.semi_border_radius()),
                            border_1,
                            border_color(theme.tile.border),
                            hover(border_color(theme.accent), text_color(theme.text.normal)),
                            text_color(theme.text.dim),
                            bg(theme.solid_bg),
                            text_size(structure.small_font_size)
                        )
                        .child(*label)
                        .id(*label)
                        .when_else(
                            is_active,
                            |el| el.bg(theme.solid_hover_bg).text_color(theme.text.normal),
                            |el| {
                                el.cursor_pointer().on_click({
                                    let field = field.clone();
                                    let settings = settings.clone();
                                    let tokio_rt = tokio_rt.clone();
                                    let mode_variants = *mode_variants;

                                    move |_, _, _| {
                                        let field = field.clone();
                                        let settings = settings.clone();

                                        tokio_rt.spawn(async move {
                                            field.set(mode_variants, &settings).await
                                        });
                                    }
                                })
                            },
                        )
                        .custom_tooltip(description, theme, structure)
                    }))
            })
            .child(div().flex_1())
            .child(cloud_button(field, settings, theme, structure)),
    )
    .child(
        tailwind_div!(
            grid,
            grid_cols(num_cols),
            gap(structure.small_gap),
            text_color(theme.text.dim),
            text_size(structure.font_size * 0.9)
        )
        .children(variants.iter().map(|(variant, label)| {
            let is_active = active_variants.contains(*variant);

            tailwind_div!(
                flex,
                items_center,
                justify_center,
                border_1,
                border_color(theme.tile.border),
                paddings(structure.small_gap),
                cursor_pointer,
                rounded(structure.inner_border_radius),
                hover(border_color(theme.accent), text_color(theme.text.normal))
            )
            .when(is_active, |el| {
                el.bg(theme.colors.success.alpha(0.1))
                    .text_color(theme.colors.success)
            })
            .child(*label)
            .id(*label)
            .on_click({
                let field = field.clone();
                let settings = settings.clone();
                let tokio_rt = tokio_rt.clone();
                let label = *label;
                let variant = *variant;

                move |_, _, _| {
                    let mut active_variants = active_variants;
                    if is_active {
                        active_variants.remove(variant);
                    } else {
                        active_variants.insert(variant);
                    }
                    tracing::trace!("clicked variant: {}", label);
                    let field = field.clone();
                    let settings = settings.clone();

                    tokio_rt.spawn(async move { field.set(active_variants, &settings).await });
                }
            })
        })),
    )
    .id(field.cloud_name)
}
