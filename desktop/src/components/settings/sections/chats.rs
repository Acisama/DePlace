use std::{collections::HashSet, sync::Arc};

use deplace_core::settings::Settings;
use gpui::{AnyElement, Context, Element, ParentElement, SharedString, Styled, div};
use macros::tailwind_div;
use tokio::runtime::Runtime;

use crate::{
    components::settings::{
        SettingsView,
        sections::{setting_toggle, spacer, subsection},
    },
    theme::{AppTheme, Structure},
};

pub fn render_chats_section(
    theme: &AppTheme,
    structure: &Structure,
    settings: &Settings,
    tokio_rt: Arc<Runtime>,
    expanded_subsections: &HashSet<SharedString>,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let _ = expanded_subsections;
    let _ = cx;

    tailwind_div!(w_full, pt(structure.small_gap), px(structure.gap))
        .child(
            subsection(
                "Indicators".into(),
                "chat-indicators".into(),
                theme,
                structure,
                expanded_subsections,
                cx,
            )
            .child(setting_toggle(
                settings,
                &settings.show_read_markers,
                theme,
                structure,
                tokio_rt.clone(),
            ))
            .child(setting_toggle(
                settings,
                &settings.send_read_markers,
                theme,
                structure,
                tokio_rt.clone(),
            ))
            .child(spacer(structure))
            .child(setting_toggle(
                settings,
                &settings.show_typing_indicators,
                theme,
                structure,
                tokio_rt.clone(),
            ))
            .child(setting_toggle(
                settings,
                &settings.send_typing_indicators,
                theme,
                structure,
                tokio_rt.clone(),
            )),
        )
        .into_any()
}
