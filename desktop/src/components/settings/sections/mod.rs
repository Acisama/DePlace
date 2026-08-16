use std::{sync::Arc, time::Duration};

use gpui::{
    Animation, AnimationExt, AnyElement, Element, ElementId, Hsla, InteractiveElement,
    ParentElement, StatefulInteractiveElement, Styled, div, px, transparent_black,
};
use gpui_component::tooltip::Tooltip;
use macros::tailwind_div;
use tokio::{runtime::Runtime, sync::watch};

use crate::{
    components::{CustomStyles, profiles::render_icon},
    theme::{AppTheme, Structure},
};
use deplace_core::settings::{MatrixSettingField, Settings};

pub(super) mod chats;

fn get_cloud_stuff(
    uses_cloud: &Option<watch::Sender<bool>>,
    theme: &AppTheme,
) -> (&'static str, Hsla, &'static str) {
    match uses_cloud.as_ref().map(|b| *b.borrow()) {
        Some(true) => (
            phosphor_svgs::icon::cloud::REGULAR,
            theme.accent,
            "This setting is synced with the cloud.",
        ),
        Some(false) => (
            phosphor_svgs::icon::cloud_slash::REGULAR,
            theme.text.dim,
            "This setting is not synced with the cloud.",
        ),
        None => (
            phosphor_svgs::icon::cloud_slash::REGULAR,
            theme.text.muted,
            "This setting can not be synced with the cloud.",
        ),
    }
}

fn render_toggle(
    settings: &Settings,
    field: &MatrixSettingField<bool>,
    theme: &AppTheme,
    structure: &Structure,
    tokio_rt: Arc<Runtime>,
) -> AnyElement {
    let (icon, color, tooltip) = get_cloud_stuff(&field.uses_cloud, theme);

    let checked = field.value();

    tailwind_div!(
        flex,
        flex_grow_1,
        px(structure.gap),
        py(structure.small_gap / 2.0),
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
    .child(
        tailwind_div!(
            flex,
            items_center,
            text_center,
            flex_row,
            gap(structure.gap),
        )
        .child(field.human_readable)
        .child(
            div()
                .id("name-tooltip")
                .child(render_icon(
                    phosphor_svgs::icon::question::REGULAR,
                    structure.font_size,
                ))
                .tooltip({
                    let description = field.description;
                    move |window, cx| Tooltip::new(description).build(window, cx)
                }),
        ),
    )
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
                    .child(
                        div()
                            .rounded_full()
                            .bg(tick_color)
                            .size(tick_size)
                            .with_animation(
                                ElementId::NamedInteger("toggle-move".into(), checked as u64),
                                Animation::new(Duration::from_secs_f64(0.15)),
                                move |this, delta| {
                                    let x = if checked {
                                        max_x * delta
                                    } else {
                                        max_x - max_x * delta
                                    };
                                    this.left(x)
                                },
                            ),
                    )
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
            .child(
                div()
                    .text_color(color)
                    .id("cloud-tooltip")
                    .child(render_icon(icon, structure.font_size * 1.2))
                    .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx)),
            ),
    )
    .into_any()
}
