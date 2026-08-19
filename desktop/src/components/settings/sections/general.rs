use std::{collections::HashSet, sync::Arc};

use deplace_core::settings::Settings;
use gpui::{AnyElement, Context, Element, ParentElement, SharedString, Styled, Window};
use macros::tailwind_div;
use tokio::runtime::Runtime;

use crate::{
    components::settings::{
        SettingsView,
        sections::{setting_dropdown, setting_toggle, spacer, subsection},
    },
    things::{AppTheme, Structure},
};

#[allow(clippy::too_many_arguments)]
pub fn render_general_section(
    theme: &AppTheme,
    structure: &Structure,
    settings: &Settings,
    tokio_rt: Arc<Runtime>,
    expanded_subsections: &HashSet<SharedString>,
    active_dropdown: &Option<&'static str>,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    tailwind_div!(w_full, pt(structure.small_gap), px(structure.gap))
        .child(
            subsection(
                "Language/Region".into(),
                "language-region-general".into(),
                theme,
                structure,
                expanded_subsections,
                cx,
            )
            .child(setting_dropdown(
                settings,
                &settings.hour_format,
                theme,
                structure,
                tokio_rt.clone(),
                active_dropdown,
                cx,
            ))
            .child(setting_dropdown(
                settings,
                &settings.date_format,
                theme,
                structure,
                tokio_rt.clone(),
                active_dropdown,
                cx,
            ))
            .child(setting_dropdown(
                settings,
                &settings.first_day_of_week,
                theme,
                structure,
                tokio_rt.clone(),
                active_dropdown,
                cx,
            ))
            .child(spacer(structure))
            .child(setting_dropdown(
                settings,
                &settings.timezone,
                theme,
                structure,
                tokio_rt.clone(),
                active_dropdown,
                cx,
            )),
        )
        .child(
            subsection(
                "Units".into(),
                "units-general".into(),
                theme,
                structure,
                expanded_subsections,
                cx,
            )
            .child(setting_dropdown(
                settings,
                &settings.data_size_unit,
                theme,
                structure,
                tokio_rt.clone(),
                active_dropdown,
                cx,
            )),
        )
        .child(
            subsection(
                "Behavior".into(),
                "behavior-general".into(),
                theme,
                structure,
                expanded_subsections,
                cx,
            )
            .child(setting_toggle(
                settings,
                &settings.minimize_to_tray,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            )),
        )
        .child(
            subsection(
                "Profile customization".into(),
                "profile-customization-general".into(),
                theme,
                structure,
                expanded_subsections,
                cx,
            )
            .child(setting_toggle(
                settings,
                &settings.play_user_theme_on_click,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            ))
            .child(setting_toggle(
                settings,
                &settings.use_banner_colors,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            ))
            .child(setting_toggle(
                settings,
                &settings.use_name_color,
                theme,
                structure,
                tokio_rt.clone(),
                window,
                cx,
            )),
        )
        .into_any()
}
