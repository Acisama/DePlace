use std::{collections::HashSet, sync::Arc};

use deplace_core::settings::{SYSTEM_MESSAGE_MODES, Settings};
use gpui::{AnyElement, Context, Element, ParentElement, SharedString, Styled, Window};
use macros::tailwind_div;
use tokio::runtime::Runtime;

use crate::{
    components::settings::{
        SettingsView,
        sections::{setting_enumset_toggles, setting_toggle, spacer, subsection},
    },
    theme::{AppTheme, Structure},
};

#[allow(clippy::too_many_arguments)]
pub fn render_chats_section(
    theme: &AppTheme,
    structure: &Structure,
    settings: &Settings,
    tokio_rt: Arc<Runtime>,
    expanded_subsections: &HashSet<SharedString>,
    _active_dropdown: &Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
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
                window,
                cx,
            ))
            .child(setting_toggle(
                settings,
                &settings.send_read_markers,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            ))
            .child(spacer(structure))
            .child(setting_toggle(
                settings,
                &settings.show_typing_indicators,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            ))
            .child(setting_toggle(
                settings,
                &settings.send_typing_indicators,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            )),
        )
        .child(
            subsection(
                "Messages".into(),
                "messages-chat".into(),
                theme,
                structure,
                expanded_subsections,
                cx,
            )
            .child(setting_toggle(
                settings,
                &settings.url_previews_default,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            ))
            .child(setting_toggle(
                settings,
                &settings.mark_pinned_messages,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            ))
            .child(spacer(structure))
            .child(setting_enumset_toggles(
                settings,
                &settings.system_messages_to_show,
                SYSTEM_MESSAGE_MODES,
                theme,
                structure,
                tokio_rt.clone(),
                5,
            )),
        )
        .into_any()
}
