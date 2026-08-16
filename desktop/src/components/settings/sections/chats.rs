use std::sync::Arc;

use deplace_core::settings::Settings;
use gpui::{AnyElement, Element, ParentElement, Styled, div};
use macros::tailwind_div;
use tokio::runtime::Runtime;

use crate::{
    components::settings::sections::render_toggle,
    theme::{AppTheme, Structure},
};

pub fn render_chats_section(
    theme: &AppTheme,
    structure: &Structure,
    settings: &Settings,
    tokio_rt: Arc<Runtime>,
) -> AnyElement {
    tailwind_div!(w_full, pt(structure.small_gap), px(structure.gap))
        .child(render_toggle(
            settings,
            &settings.show_read_markers,
            theme,
            structure,
            tokio_rt.clone(),
        ))
        .into_any()
}
