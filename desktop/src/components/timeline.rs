use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use deplace_core::{
    helpers::format_date_divider, matrix_api::timeline::DisplayString, state::MembershipMap,
};
use gpui::{
    AnyElement, Div, Element, InteractiveElement, ParentElement, Pixels, Styled, div, hsla, px,
};
use gpui_component::red_600;
use matrix_sdk::ruma::{OwnedUserId, RoomId, UserId, events::rtc::notification::CallIntent};
use matrix_sdk_ui::timeline::{
    EventTimelineItem, MemberProfileChange, MembershipChange, RoomMembershipChange, TimelineItem,
    TimelineItemContent, TimelineItemKind, VirtualTimelineItem,
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
) -> AnyElement {
    match item.kind() {
        TimelineItemKind::Virtual(virt) => render_virtual_timeline_item(virt, prev, next, theme),
        TimelineItemKind::Event(event) => render_timeline_event(
            event,
            item.unique_id().0.clone(),
            room_id,
            theme,
            membership_map,
            avatar_cache,
        ),
    }
}

fn render_virtual_timeline_item(
    virt: &VirtualTimelineItem,
    _prev: Option<Arc<TimelineItem>>,
    _next: Option<Arc<TimelineItem>>,
    theme: &AppTheme,
) -> AnyElement {
    let content = match virt {
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
    };

    content.into_any()
}

fn render_timeline_event(
    event: &EventTimelineItem,
    id: String,
    room_id: &RoomId,
    theme: &AppTheme,
    membership_map: &MembershipMap,
    avatar_cache: &AvatarCache,
) -> AnyElement {
    let sender_id = event.sender();
    let member = membership_map.get(room_id).and_then(|m| m.get(sender_id));

    let sender_div_params = |icon_size: Pixels, name_size: Pixels| {
        div()
            .flex()
            .items_center()
            .gap(theme.gap)
            .mr(theme.small_gap)
            .child(member.render_avatar(icon_size, icon_size / 2.0, avatar_cache))
            .child(member.render_name(name_size))
    };

    let member_avatar = |user_id: &UserId, size: Pixels| {
        let member = membership_map.get(room_id).and_then(|m| m.get(user_id));
        member.render_avatar(size, size / 2.0, avatar_cache)
    };

    let member_name = |user_id: &UserId, size: Pixels| {
        let member = membership_map.get(room_id).and_then(|m| m.get(user_id));
        member.render_name(size)
    };

    let sender_div = || sender_div_params(px(20.0), px(16.0));

    let content = match event.content() {
        TimelineItemContent::CallInvite => div(),
        TimelineItemContent::FailedToParseMessageLike { event_type, error } => render_error(
            format!("Failed to parse event of type {}: {}", event_type, error),
            sender_div,
            theme,
        ),
        TimelineItemContent::FailedToParseState {
            event_type,
            state_key,
            error,
        } => render_error(
            format!(
                "Failed to parse state of type {} with key {}: {}",
                event_type, state_key, error
            ),
            sender_div,
            theme,
        ),
        TimelineItemContent::MembershipChange(change) => {
            render_membership_change(change, sender_div)
        }
        TimelineItemContent::MsgLike(msg) => div().child(format!("{:?}", msg)),
        TimelineItemContent::OtherState(other) => div(),
        TimelineItemContent::ProfileChange(change) => render_profile_change(change, sender_div),
        TimelineItemContent::RtcNotification {
            call_intent,
            declined_by,
        } => render_rtc_notification(
            call_intent,
            declined_by,
            theme,
            sender_div,
            |id| member_avatar(id, px(20.0)),
            |id| member_name(id, px(16.0)),
        ),
    };

    div()
        .w_full()
        .flex()
        .items_start()
        .border_1()
        .child(content)
        .text_color(theme.text.normal)
        .rounded(theme.inner_border_radius)
        .hover(|style| {
            style
                .border_color(theme.tile.border)
                .bg(hsla(0.0, 0.0, 0.0, 0.2))
        })
        .id(id)
        .into_any()
}

fn render_rtc_notification(
    call_intent: &Option<CallIntent>,
    declined_by: &[OwnedUserId],
    theme: &AppTheme,
    sender_div: impl Fn() -> Div,
    member_avatar: impl Fn(&UserId) -> AnyElement,
    member_name: impl Fn(&UserId) -> Div,
) -> Div {
    let intent_string = if let Some(intent) = call_intent {
        match intent {
            CallIntent::Video => "started a video call",
            CallIntent::Audio | _ => "started an audio call",
        }
    } else {
        "started a call"
    };

    let declined_divs = declined_by.iter().map(|id| {
        div()
            .flex()
            .items_center()
            .gap(theme.small_gap)
            .child(member_avatar(id))
            .child(member_name(id))
    });

    div()
        .flex()
        .child(sender_div())
        .child(intent_string)
        .children(declined_divs)
}

fn render_profile_change(
    profile_change: &MemberProfileChange,
    sender_div: impl Fn() -> Div,
) -> Div {
    let text = profile_change.display_string();

    div().flex().child(sender_div()).child(text)
}

fn render_membership_change(change: &RoomMembershipChange, sender_div: impl Fn() -> Div) -> Div {
    let text = if let Some(membership) = &change.change() {
        match membership {
            MembershipChange::None => "had no membership change",
            MembershipChange::Banned => "was banned",
            MembershipChange::Joined => "joined the room",
            MembershipChange::Invited => "was invited",
            MembershipChange::Left => "left the room",
            MembershipChange::Kicked => "was kicked",
            MembershipChange::Error => "had a membership change",
            MembershipChange::InvitationAccepted => "accepted the invitation",
            MembershipChange::InvitationRejected => "rejected the invitation",
            MembershipChange::InvitationRevoked => "had their invitation revoked",
            MembershipChange::KickedAndBanned => "was kicked and banned",
            MembershipChange::KnockAccepted => "accepted the knock",
            MembershipChange::KnockDenied => "denied the knock",
            MembershipChange::KnockRetracted => "retracted the knock",
            MembershipChange::Knocked => "knocked on the door",
            MembershipChange::NotImplemented => "had a membership change",
            MembershipChange::Unbanned => "was unbanned",
        }
    } else {
        "changed membership"
    };

    div().flex().child(sender_div()).child(text)
}

fn render_error(text: String, sender_div: impl Fn() -> Div, theme: &AppTheme) -> Div {
    div()
        .flex()
        .child(sender_div())
        .child(text)
        .text_color(theme.colors.error)
}
