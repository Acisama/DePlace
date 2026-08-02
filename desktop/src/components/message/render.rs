use deplace_core::state::MembershipMap;
use gpui::{
    AnyElement, Element, ElementId, InteractiveElement, LinearColorStop, ParentElement, Pixels,
    Styled, div, linear_gradient, prelude::FluentBuilder, relative, transparent_black,
};
use gpui_component::{Colorize, red_600};
use macros::tailwind_div;
use matrix_sdk::ruma::RoomId;

use crate::{
    components::{
        AvatarCache, CustomStyles, MemberRenderer,
        message::{
            CachedEventContent, CachedSendState, CachedTimelineEvent, CachedTimelineItem,
            CachedTimelineItemKind,
        },
    },
    theme::AppTheme,
};

impl CachedTimelineItem {
    pub fn render(
        &self,
        prev: Option<&Self>,
        next: Option<&Self>,
        theme: &AppTheme,
        curent_room_id: &RoomId,
        map: &MembershipMap,
        avatar_cache: &AvatarCache,
    ) -> AnyElement {
        let divider_width = theme.structure.divider_width;

        let content = match &self.kind {
            // TODO: Merge with read marker if adjacent
            // fn is_read_marker(item: Option<Arc<TimelineItem>>) -> bool {
            //     item.and_then(|i| i.as_virtual().cloned())
            //         .is_some_and(|v| matches!(v, VirtualTimelineItem::ReadMarker))
            // }
            CachedTimelineItemKind::DateDivider(date) => {
                tailwind_div!(w_full, flex, items_center, gap(theme.gap))
                    .child(tailwind_div!(h(divider_width), flex_1 bg(theme.tile.border)))
                    .child(tailwind_div!(text_color(theme.text.muted)).child(date.clone()))
                    .child(tailwind_div!(h(divider_width) flex_1 bg(theme.tile.border)))
                    .into_any()
            }
            CachedTimelineItemKind::ReadMarker => tailwind_div!(w_full, items_center, flex, h_1)
                .child(tailwind_div!(h(divider_width), flex_1 bg(theme.accent)))
                .into_any(),
            CachedTimelineItemKind::TimelineStart => {
                tailwind_div!(w_full, h_20, bg(red_600())).into_any()
            }
            CachedTimelineItemKind::Event(event) => event.render(
                self.id(),
                theme,
                curent_room_id,
                map,
                avatar_cache,
                prev,
                next,
            ),
        };

        tailwind_div!(w_full).child(content).into_any()
    }
}

#[allow(clippy::too_many_arguments)]
impl CachedTimelineEvent {
    fn render(
        &self,
        id: ElementId,
        theme: &AppTheme,
        current_room_id: &RoomId,
        map: &MembershipMap,
        avatar_cache: &AvatarCache,
        prev: Option<&CachedTimelineItem>,
        next: Option<&CachedTimelineItem>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let chat = &theme.structure.chat;

        let show_highlight = self.flags.is_highlighted;
        let mut is_system_message = false;

        let member = map.get(current_room_id).and_then(|m| m.get(&*self.sender));

        let sender_avatar =
            move |size: Pixels| member.render_avatar(size, size / 2.0, avatar_cache);
        let sender_name = move |size: Pixels| member.render_name(size);

        let mut show_header = false;
        let mut pad_bottom = false;

        let content = match &self.content {
            CachedEventContent::FailedToParseMessageLike(text)
            | CachedEventContent::FailedToParseState(text) => {
                tailwind_div!(text_color(colors.error)).child(text.clone())
            }
            CachedEventContent::SystemMessage(msg) => {
                if let Some(text) = msg.text() {
                    is_system_message = true;
                    tailwind_div!(
                        text_color(theme.text.dim),
                        items_center,
                        flex,
                        flex_1,
                        justify_center
                    )
                    .child(sender_avatar(chat.small_icon_size))
                    .child(" ")
                    .child(sender_name(chat.text_size))
                    .child(" ")
                    .child(text)
                } else {
                    return div().into_any();
                }
            }
            CachedEventContent::UserMessage(msg) => {
                show_header = msg.in_reply_to.is_some()
                    || prev
                        .map(|item| {
                            if let CachedTimelineItemKind::Event(CachedTimelineEvent {
                                content: CachedEventContent::UserMessage(_),
                                sender,
                                timestamp,
                                ..
                            }) = &item.kind
                            {
                                timestamp.abs_diff(self.timestamp) > 300 || sender != &self.sender
                            } else {
                                true
                            }
                        })
                        .unwrap_or(true);

                pad_bottom = next.is_some_and(|item| {
                    matches!(&item.kind,
                        CachedTimelineItemKind::Event(next_event)
                        if next_event.timestamp.abs_diff(self.timestamp) > 300
                            || next_event.sender != self.sender)
                });
                tailwind_div!(text_color(theme.text.normal), line_height(relative(1.0)))
                    .child(msg.body.clone().unwrap_or_default())
            }
        };

        let highlight_color = if show_highlight {
            Some(theme.accent)
        } else {
            None
        };

        let (bg, hover_bg) = if let Some(color) = highlight_color {
            let color = color.alpha(0.4);
            (
                linear_gradient(
                    90.0,
                    LinearColorStop {
                        color,
                        percentage: 0.0,
                    },
                    LinearColorStop {
                        color: transparent_black(),
                        percentage: 1.0,
                    },
                ),
                linear_gradient(
                    90.0,
                    LinearColorStop {
                        color: color.darken(0.4),
                        percentage: 0.0,
                    },
                    LinearColorStop {
                        color: transparent_black(),
                        percentage: 1.0,
                    },
                ),
            )
        } else {
            (transparent_black().into(), theme.tile.background.into())
        };

        let icon_size = theme.structure.chat.icon_size;
        let col_width = icon_size + 2.0 * theme.gap;

        let text_color = self
            .state
            .as_ref()
            .map(|s| match s {
                CachedSendState::NotSentYet { .. } => theme.text.dim,
                CachedSendState::SendingFailed { .. } => colors.error,
                CachedSendState::Sent { .. } => theme.text.normal,
            })
            .unwrap_or(theme.text.normal);

        let mt = if show_header {
            theme.small_gap + Pixels::from(2.0)
        } else {
            Pixels::ZERO
        };
        let mb = if pad_bottom {
            theme.small_gap + Pixels::from(2.0)
        } else {
            Pixels::ZERO
        };

        tailwind_div!(
            w_full,
            border_transparent,
            rounded(theme.small_gap),
            group("message"),
            bg(bg),
            hover(border_color(theme.tile.border), bg(hover_bg)),
            flex,
            py(theme.small_gap),
            mt(mt),
            mb(mb),
            flex_row,
            text_color(text_color),
        )
        .when(self.flags.contains_only_emojis, |el| {
            el.text_size(chat.text_size * 2.0)
        })
        .when(!self.flags.contains_only_emojis, |el| {
            el.text_size(chat.text_size)
        })
        .id(id.clone())
        .child(
            tailwind_div!(w(col_width), px(theme.gap))
                .when(show_header, |el| el.child(sender_avatar(chat.icon_size)))
                .when(!show_header, |el| {
                    el.child(
                        tailwind_div!(
                            text_color(transparent_black()),
                            text_size(chat.small_text_size)
                        )
                        .id(id)
                        .group_hover("message", |style| style.text_color(theme.text.muted))
                        .child(self.sent_time.clone()),
                    )
                }),
        )
        .child(
            tailwind_div!(flex, size_full, flex_col, gap(theme.small_gap))
                .when(show_header && !is_system_message, |el| {
                    el.child(sender_name(chat.text_size))
                })
                .child(content),
        )
        .into_any()
    }
}
