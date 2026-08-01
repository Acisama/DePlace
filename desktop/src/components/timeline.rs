use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use deplace_core::{helpers::format_date_divider, state::MembershipMap};
use gpui::{Div, InteractiveElement, ParentElement, Styled, div, hsla, px, rgba};
use gpui_component::red_600;
use matrix_sdk::ruma::{RoomId, events::rtc::notification::CallIntent};
use matrix_sdk_ui::timeline::{
    EventTimelineItem, TimelineItem, TimelineItemContent, TimelineItemKind, VirtualTimelineItem,
};

use crate::{
    components::{AvatarCache, MemberRenderer},
    theme::AppTheme,
};

pub fn render_timeline_item(
    item: Arc<TimelineItem>,
    prev: Option<Arc<TimelineItem>>,
    next: Option<Arc<TimelineItem>>,
    theme: &AppTheme,
    room_id: &RoomId,
    membership_map: &MembershipMap,
    avatar_cache: &AvatarCache,
) -> Div {
    match item.kind() {
        TimelineItemKind::Virtual(virt) => render_virtual_timeline_item(virt, prev, next, theme),
        TimelineItemKind::Event(event) => {
            render_timeline_event(event, room_id, theme, membership_map, avatar_cache)
        }
    }
}

fn render_virtual_timeline_item(
    virt: &VirtualTimelineItem,
    _prev: Option<Arc<TimelineItem>>,
    _next: Option<Arc<TimelineItem>>,
    theme: &AppTheme,
) -> Div {
    match virt {
        VirtualTimelineItem::ReadMarker => div().w_full().items_center().flex().h_1().child(
            div()
                .h(theme.structure.divider_width)
                .flex_1()
                .bg(theme.accent),
        ),
        VirtualTimelineItem::TimelineStart => div().w_full().h_20().bg(red_600()),
        VirtualTimelineItem::DateDivider(date) => {
            // TODO: Merge with read marker if adjacent
            // fn is_read_marker(item: Option<Arc<TimelineItem>>) -> bool {
            //     item.and_then(|i| i.as_virtual().cloned())
            //         .is_some_and(|v| matches!(v, VirtualTimelineItem::ReadMarker))
            // }

            let secs: u64 = date.as_secs().into();
            let date = Local
                .timestamp_opt(secs as i64, 0)
                .latest()
                .unwrap_or_else(|| DateTime::UNIX_EPOCH.with_timezone(&Local));

            div()
                .w_full()
                .flex()
                .items_center()
                .gap(theme.gap)
                .child(
                    div()
                        .h(theme.structure.divider_width)
                        .flex_1()
                        .bg(theme.tile.border),
                )
                .child(
                    div()
                        .text_color(theme.text.muted)
                        .child(format_date_divider(date)),
                )
                .child(
                    div()
                        .h(theme.structure.divider_width)
                        .flex_1()
                        .bg(theme.tile.border),
                )
        }
    }
}

fn render_timeline_event(
    event: &EventTimelineItem,
    room_id: &RoomId,
    theme: &AppTheme,
    membership_map: &MembershipMap,
    avatar_cache: &AvatarCache,
) -> Div {
    let sender_id = event.sender();
    let member = membership_map.get(room_id).and_then(|m| m.get(sender_id));

    let sender_div = || {
        div()
            .flex()
            .items_center()
            .gap(theme.gap)
            .mr(theme.small_gap)
            .child(member.render_avatar(px(20.0), px(10.0), avatar_cache))
            .child(member.render_name(px(16.0)))
    };

    let content = match event.content() {
        TimelineItemContent::CallInvite => div(),
        TimelineItemContent::FailedToParseMessageLike { event_type, error } => {
            div().child(format!("{:?}", error))
        }
        TimelineItemContent::FailedToParseState {
            event_type,
            state_key,
            error,
        } => div(),
        TimelineItemContent::MembershipChange(change) => div(),
        TimelineItemContent::MsgLike(msg) => div().child(format!("{:?}", msg)),
        TimelineItemContent::OtherState(other) => div(),
        TimelineItemContent::ProfileChange(change) => div(),
        TimelineItemContent::RtcNotification {
            call_intent,
            declined_by,
        } => {
            let intent_string = if let Some(intent) = call_intent {
                match intent {
                    CallIntent::Video => "started a video call",
                    CallIntent::Audio | _ => "started an audio call",
                }
            } else {
                "started a call"
            };

            let declined_divs = declined_by.iter().map(|id| {
                let member = membership_map.get(room_id).and_then(|m| m.get(id));

                div()
                    .flex()
                    .items_center()
                    .gap(theme.small_gap)
                    .child(member.render_avatar(px(20.0), px(10.0), avatar_cache))
                    .child(member.render_name(px(16.0)))
            });

            div()
                .flex()
                .child(sender_div())
                .child(intent_string)
                .children(declined_divs)
        }
    };

    div()
        .w_full()
        .flex()
        .items_start()
        .border_1()
        .child(content)
        .hover(|style| {
            style
                .border_color(theme.tile.border)
                .bg(hsla(0.0, 0.0, 0.0, 0.2))
        })
}
