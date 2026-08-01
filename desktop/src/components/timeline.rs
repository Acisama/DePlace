use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use deplace_core::helpers::format_date_divider;
use gpui::{Div, ParentElement, Styled, div};
use gpui_component::{gray_200, red_600};
use matrix_sdk_ui::timeline::{TimelineItem, TimelineItemKind, VirtualTimelineItem};

use crate::theme::AppTheme;

pub fn render_timeline_item(item: Arc<TimelineItem>, theme: &AppTheme) -> Div {
    match item.kind() {
        TimelineItemKind::Virtual(virt) => render_virtual_timeline_item(virt, theme),
        TimelineItemKind::Event(event) => div().child(format!("{:?}", event)),
    }
}

fn render_virtual_timeline_item(virt: &VirtualTimelineItem, theme: &AppTheme) -> Div {
    match virt {
        VirtualTimelineItem::ReadMarker => div().w_full().h_1().bg(gray_200()),
        VirtualTimelineItem::TimelineStart => div().w_full().h_1().bg(red_600()),
        VirtualTimelineItem::DateDivider(date) => {
            let secs: u64 = date.as_secs().into();
            let date = Local
                .timestamp_opt(secs as i64, 0)
                .latest()
                .unwrap_or_else(|| DateTime::UNIX_EPOCH.with_timezone(&Local));

            div()
                .flex()
                .items_center()
                .gap(theme.gap)
                .child(div().flex_1().bg(theme.tile.border))
                .child(
                    div()
                        .text_color(theme.text.muted)
                        .child(format_date_divider(date)),
                )
                .child(div().flex_1().bg(theme.tile.border))
        }
    }
}
