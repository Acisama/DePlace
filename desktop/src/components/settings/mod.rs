use std::{collections::HashSet, sync::Arc};

use deplace_core::settings::{Settings, SettingsSection};
use gpui::{
    AnyElement, App, Context, Element, EventEmitter, FocusHandle, Focusable, InteractiveElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, actions, div,
    prelude::FluentBuilder, px, relative,
};
use gpui_component::{StyledExt, scroll::ScrollableElement};
use macros::tailwind_div;
use tokio::runtime::Runtime;

use crate::{
    components::{
        CustomStyles, close_button, floating_tile,
        profiles::render_icon,
        settings::sections::{chats::render_chats_section, general::render_general_section},
    },
    theme::{AppTheme, DeplaceThings, Structure},
    watch_bridge::notify_on_change,
};

mod sections;

actions!(settings, [Close]);

pub struct SettingsView {
    settings: Settings,
    tokio_rt: Arc<Runtime>,

    active_section: UiSettingsSection,
    expanded_subsections: HashSet<SharedString>,
    active_dropdown: Option<&'static str>,

    focus: FocusHandle,
}

impl SettingsView {
    pub fn new(cx: &mut Context<Self>, settings: Settings, tokio_rt: Arc<Runtime>) -> Self {
        let focus = cx.focus_handle();

        notify_on_change(settings.watch_any_change(), cx);

        Self {
            settings,
            tokio_rt,

            active_section: PROFILE_SECTION,
            expanded_subsections: HashSet::new(),
            active_dropdown: None,

            focus,
        }
    }
}

impl EventEmitter<Close> for SettingsView {}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[derive(Clone)]
struct UiSettingsSection {
    title: &'static str,
    id: SettingsSection,
    icon: &'static str,
    #[allow(clippy::type_complexity)]
    render_fn: fn(
        &AppTheme,
        &Structure,
        &Settings,
        Arc<Runtime>,
        &HashSet<SharedString>,
        &Option<&'static str>,
        &mut Window,
        &mut Context<SettingsView>,
    ) -> AnyElement,
}

#[derive(Clone)]
enum SettingsItem {
    Section(UiSettingsSection),
    Divider,
}

impl SettingsItem {
    fn render_column_item(
        &self,
        theme: &AppTheme,
        structure: &Structure,
        cx: &mut Context<SettingsView>,
        active_section: &UiSettingsSection,
    ) -> AnyElement {
        match self {
            SettingsItem::Divider => tailwind_div!(
                bg(theme.tile.border),
                h(structure.divider_width),
                mx(structure.small_gap)
            )
            .into_any(),
            SettingsItem::Section(section) => {
                let is_active = section.id == active_section.id;

                tailwind_div!(
                    flex,
                    items_center,
                    text_left,
                    text_color(theme.text.dim),
                    hover(
                        text_color(theme.text.normal),
                        border_color(theme.tile.border)
                    ),
                    gap(structure.small_gap),
                    mx(structure.small_gap),
                    rounded(structure.semi_border_radius()),
                    cursor_pointer,
                    p(structure.small_gap),
                    line_height(relative(1.0)),
                    border_transparent,
                )
                .when(is_active, |el| {
                    el.border_color(theme.tile.border)
                        .bg(theme.solid_hover_bg)
                        .text_color(theme.text.normal)
                        .cursor_default()
                })
                .id(section.id.id())
                .child(render_icon(section.icon, structure.font_size * 1.2))
                .child(section.title)
                .when(!is_active, |el| {
                    el.on_click(cx.listener({
                        let section = section.clone();
                        move |view, _event, _window, cx| {
                            view.active_section = section.clone();
                            cx.notify();
                        }
                    }))
                })
                .into_any()
            }
        }
    }
}

const SETTINGS_SECTIONS: &[SettingsItem] = &[
    SettingsItem::Divider,
    SettingsItem::Section(UiSettingsSection {
        title: "General",
        id: SettingsSection::General,
        icon: phosphor_svgs::icon::sliders::FILL,
        render_fn: render_general_section,
    }),
    SettingsItem::Section(UiSettingsSection {
        title: "Appearance",
        id: SettingsSection::Appearance,
        icon: phosphor_svgs::icon::palette::REGULAR,
        render_fn: |_, _, _, _, _, _, _, _| div().into_any(),
    }),
    SettingsItem::Section(UiSettingsSection {
        title: "Audio",
        id: SettingsSection::Audio,
        icon: phosphor_svgs::icon::headphones::FILL,
        render_fn: |_, _, _, _, _, _, _, _| div().into_any(),
    }),
    SettingsItem::Section(UiSettingsSection {
        title: "Chats",
        id: SettingsSection::Chats,
        icon: phosphor_svgs::icon::chats::FILL,
        render_fn: render_chats_section,
    }),
    SettingsItem::Divider,
    SettingsItem::Section(UiSettingsSection {
        title: "Updates",
        id: SettingsSection::Updates,
        icon: phosphor_svgs::icon::arrows_clockwise::FILL,
        render_fn: |_, _, _, _, _, _, _, _| div().into_any(),
    }),
];

const PROFILE_SECTION: UiSettingsSection = UiSettingsSection {
    title: "Profile",
    id: SettingsSection::Profile,
    icon: phosphor_svgs::icon::pencil_simple::FILL,
    render_fn: |_, _, _, _, _, _, _, _| div().into_any(),
};

impl Render for SettingsView {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        let theme = cx.app_theme().clone();
        let structure = cx.structure().clone();
        let theme = &theme;
        let structure = &structure;

        let active_section = &self.active_section;

        tailwind_div!(
            flex,
            flex_row,
            gap(structure.small_gap),
            w(structure.settings.full_width),
            h(structure.settings.full_height)
        )
        .id("settings")
        .key_context("Settings")
        .track_focus(&self.focus)
        .on_click(|_event, _window, cx| {
            cx.stop_propagation();
        })
        .on_action(cx.listener(|_, _: &Close, _, cx| cx.emit(Close)))
        .child(
            floating_tile(theme, structure)
                .rounded_l(structure.outer_border_radius)
                .rounded_r(structure.inner_border_radius)
                .w(structure.settings.section_column_width)
                .gap(structure.small_gap)
                .h_full()
                .flex()
                .flex_col()
                .overflow_y_scrollbar()
                .child(tailwind_div!(w_full, h(px(20.0))))
                .children(
                    SETTINGS_SECTIONS
                        .iter()
                        .map(|s| s.render_column_item(theme, structure, cx, active_section)),
                ),
        )
        .child(
            tailwind_div!(
                flex,
                flex_col,
                gap(structure.small_gap),
                h_full,
                flex_grow_1
            )
            .child(
                floating_tile(theme, structure)
                    .h(structure.header.height)
                    .child(active_section.title)
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl((structure.header.height - structure.font_size * 1.2) / 2.0)
                    .pr((structure.header.height - structure.font_size * 1.2) / 2.0
                        - structure.small_gap / 2.0)
                    .text_size(structure.font_size * 1.2)
                    .font_extrabold()
                    .text_color(theme.text.normal)
                    .rounded(structure.inner_border_radius)
                    .rounded_tr(structure.outer_border_radius)
                    .child(
                        close_button(theme, "settings-close", structure.header.icon_size)
                            .p(structure.small_gap / 2.0)
                            .rounded(structure.semi_border_radius())
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Close))),
                    ),
            )
            .child(
                floating_tile(theme, structure)
                    .flex_grow_1()
                    .rounded(structure.inner_border_radius)
                    .rounded_br(structure.outer_border_radius)
                    .child((active_section.render_fn)(
                        theme,
                        structure,
                        &self.settings,
                        self.tokio_rt.clone(),
                        &self.expanded_subsections,
                        &self.active_dropdown,
                        window,
                        cx,
                    )),
            ),
        )
        .into_any()
    }
}
