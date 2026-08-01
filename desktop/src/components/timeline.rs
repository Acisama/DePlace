use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use deplace_core::{
    get_change,
    helpers::format_date_divider,
    matrix_api::timeline::{DisplayString, EventChange, get_current_and_prev},
    state::MembershipMap,
};
use gpui::{
    AnyElement, Div, Element, InteractiveElement, ParentElement, Pixels, Styled, div, hsla, px,
};
use gpui_component::red_600;
use matrix_sdk::ruma::{
    OwnedUserId, RoomId, UserId,
    events::{StateEventContentChange, room::message::MessageType, rtc::notification::CallIntent},
};
use matrix_sdk_ui::timeline::{
    AnyOtherStateEventContentChange, EventTimelineItem, MemberProfileChange, MembershipChange,
    Message, MsgLikeContent, MsgLikeKind, OtherState, RoomMembershipChange, TimelineItem,
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
        TimelineItemContent::MsgLike(msg) => render_msg_like(msg, sender_div, theme),
        TimelineItemContent::OtherState(other) => {
            render_other_state(other, sender_div).text_color(theme.text.dim)
        }
        TimelineItemContent::ProfileChange(change) => {
            render_profile_change(change, sender_div).text_color(theme.text.dim)
        }
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
        )
        .text_color(theme.text.dim),
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

fn render_other_state(other: &OtherState, sender_div: impl Fn() -> Div) -> Div {
    match other.content() {
        AnyOtherStateEventContentChange::PolicyRuleRoom(_) => {
            div().child(sender_div()).child("changed the room's policy")
        }
        AnyOtherStateEventContentChange::PolicyRuleServer(_) => div()
            .child(sender_div())
            .child("changed the server's policy"),
        AnyOtherStateEventContentChange::PolicyRuleUser(_) => {
            div().child(sender_div()).child("changed their policy")
        }
        AnyOtherStateEventContentChange::RoomAvatar(content) => {
            let change = get_change!(content, |c| c.url.clone());
            let text = change.display_string("the room's avatar");
            div().child(sender_div()).child(text)
        }
        AnyOtherStateEventContentChange::RoomCanonicalAlias(content) => {
            let change = get_change!(content, |c| c.alias.clone());
            let text = change.display_string("the room's canonical alias");

            div().child(sender_div()).child(text)
        }
        AnyOtherStateEventContentChange::RoomCreate(_) => {
            div().child(sender_div()).child("created the room")
        }
        AnyOtherStateEventContentChange::RoomEncryption(_) => {
            div().child(sender_div()).child("enabled room encryption")
        }
        AnyOtherStateEventContentChange::RoomGuestAccess(content) => {
            let change = get_current_and_prev(
                content,
                |c| Some(c.guest_access.clone()),
                |c| c.guest_access.clone(),
            );
            let text = change.display_string("the room's guest access");
            div().child(sender_div()).child(text)
        }
        AnyOtherStateEventContentChange::RoomHistoryVisibility(content) => {
            let change = get_change!(content, |c| Some(c.history_visibility.clone()));
            let text = change.display_string("the room's history visibility");
            div().child(sender_div()).child(text)
        }
        AnyOtherStateEventContentChange::RoomJoinRules(content) => {
            let change = get_change!(content, |c| Some(c.join_rule.clone()));
            let text = change
                .display_string_with_render_fn(|c| c.as_str().to_string(), "the room's join rules");
            div().child(sender_div()).child(text)
        }
        AnyOtherStateEventContentChange::RoomName(content) => {
            let change =
                get_current_and_prev(content, |c| Some(c.name.clone()), |c| c.name.clone());
            let text = change.display_string("the room's name");
            div().child(sender_div()).child(text)
        }
        AnyOtherStateEventContentChange::RoomPinnedEvents(_) => div()
            .child(sender_div())
            .child("changed the room's pinned events"),
        AnyOtherStateEventContentChange::RoomPowerLevels(_) => div()
            .child(sender_div())
            .child("changed the room's power levels"),
        AnyOtherStateEventContentChange::RoomServerAcl(_) => div()
            .child(sender_div())
            .child("changed the room's server ACL"),
        AnyOtherStateEventContentChange::RoomThirdPartyInvite(_) => div()
            .child(sender_div())
            .child("changed the room's third party invite"),
        AnyOtherStateEventContentChange::RoomTombstone(_) => div()
            .child(sender_div())
            .child("changed the room's tombstone"),
        AnyOtherStateEventContentChange::RoomTopic(content) => {
            let change =
                get_current_and_prev(content, |c| Some(c.topic.clone()), |c| c.topic.clone());
            let text = change.display_string("the room's topic");
            div().child(sender_div()).child(text)
        }
        AnyOtherStateEventContentChange::SpaceChild(_) => {
            div().child(sender_div()).child("changed the space's child")
        }
        AnyOtherStateEventContentChange::SpaceParent(_) => div()
            .child(sender_div())
            .child("changed the space's parent"),
        _ => div().child(sender_div()).child("changed the room's "),
    }
}

fn render_msg_like(msg: &MsgLikeContent, _sender_div: impl Fn() -> Div, theme: &AppTheme) -> Div {
    // TODO: Render reactions
    // TODO: Render reply header

    let content = match &msg.kind {
        MsgLikeKind::Redacted => div().text_color(theme.text.dim).child("redacted"),
        // TODO: Render live location
        MsgLikeKind::LiveLocation(_) => div()
            .text_color(theme.colors.warning)
            .child("Locations are not supported yet"),
        MsgLikeKind::Poll(_) => div()
            .text_color(theme.colors.warning)
            .child("Polls are not supported yet"),
        MsgLikeKind::UnableToDecrypt(_) => div()
            .text_color(theme.colors.warning)
            .child("Unable to decrypt"),
        MsgLikeKind::Other(kind) => div().text_color(theme.colors.warning).child(format!(
            "This message kind is not supported yet: {:?}",
            kind
        )),
        MsgLikeKind::Sticker(_) => div()
            .text_color(theme.colors.warning)
            .child("Stickers are not supported yet"),
        MsgLikeKind::Message(msg) => render_message(msg, theme),
    };

    div().child(content)
}

fn render_message(msg: &Message, theme: &AppTheme) -> Div {
    let msg_type = &msg.msgtype();
    let content = match msg_type {
        MessageType::Audio(_) => div()
            .text_color(theme.text.dim)
            .child("Audio messages are not supported yet"),
        MessageType::Emote(_) => div()
            .text_color(theme.text.dim)
            .child("Emotes are not supported yet"),
        MessageType::File(_) => div()
            .text_color(theme.text.dim)
            .child("Files are not supported yet"),
        MessageType::Image(_) => div()
            .text_color(theme.text.dim)
            .child("Images are not supported yet"),
        MessageType::Location(_) => div()
            .text_color(theme.text.dim)
            .child("Locations are not supported yet"),
        MessageType::Notice(_) => div()
            .text_color(theme.text.dim)
            .child("Notices are not supported yet"),
        MessageType::ServerNotice(_) => div()
            .text_color(theme.text.dim)
            .child("Server notices are not supported yet"),
        MessageType::Text(text) => div().text_color(theme.text.normal).child(text.body.clone()),
        MessageType::VerificationRequest(_) => div()
            .text_color(theme.text.dim)
            .child("Verification requests are not supported yet"),
        MessageType::Video(_) => div()
            .text_color(theme.text.dim)
            .child("Video messages are not supported yet"),
        _ => div().text_color(theme.text.dim).child(format!(
            "Unsupported message type: {:?}",
            msg_type.msgtype()
        )),
    };

    div().child(content)
}
