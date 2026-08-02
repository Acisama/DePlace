use deplace_core::state::MembershipMap;
use gpui::{
    AnyElement, Div, Element, InteractiveElement, ParentElement, Styled, div, transparent_black,
};
use gpui_component::red_600;
use macros::tailwind_div;
use matrix_sdk::ruma::RoomId;

use crate::{
    components::{
        AvatarCache, CustomStyles,
        message::{
            CachedEventContent, CachedTimelineEvent, CachedTimelineItem, CachedTimelineItemKind,
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
    ) -> Option<AnyElement> {
        let divider_width = theme.structure.divider_width;

        let (hover_bg, hover_border) = if self.has_hover_effect() {
            (theme.tile.background, theme.tile.border)
        } else {
            (transparent_black(), transparent_black())
        };

        let content = match &self.kind {
            // TODO: Merge with read marker if adjacent
            // fn is_read_marker(item: Option<Arc<TimelineItem>>) -> bool {
            //     item.and_then(|i| i.as_virtual().cloned())
            //         .is_some_and(|v| matches!(v, VirtualTimelineItem::ReadMarker))
            // }
            CachedTimelineItemKind::DateDivider(date) => Some(
                tailwind_div!(w_full, flex, items_center, gap(theme.gap))
                    .child(tailwind_div!(h(divider_width), flex_1 bg(theme.tile.border)))
                    .child(tailwind_div!(text_color(theme.text.muted)).child(date.clone()))
                    .child(tailwind_div!(h(divider_width) flex_1 bg(theme.tile.border))),
            ),
            CachedTimelineItemKind::ReadMarker => Some(
                tailwind_div!(w_full, items_center, flex, h_1)
                    .child(tailwind_div!(h(divider_width), flex_1 bg(theme.accent))),
            ),
            CachedTimelineItemKind::TimelineStart => {
                Some(tailwind_div!(w_full, h_20, bg(red_600())))
            }
            CachedTimelineItemKind::Event(event) => {
                event.render(theme, curent_room_id, map, avatar_cache)
            }
        }?;

        Some(
            tailwind_div!(
                border_transparent,
                rounded(theme.inner_border_radius),
                hover(border_color(hover_border), bg(hover_bg))
            )
            .id(self.id.clone())
            .w_full()
            .child(content)
            .into_any(),
        )
    }
}

impl CachedTimelineEvent {
    fn render(
        &self,
        theme: &AppTheme,
        current_room_id: &RoomId,
        map: &MembershipMap,
        avatar_cache: &AvatarCache,
    ) -> Option<Div> {
        let colors = &theme.colors;

        let content = match &self.content {
            CachedEventContent::FailedToParseMessageLike(text)
            | CachedEventContent::FailedToParseState(text) => {
                tailwind_div!(text_color(colors.error)).child(text.clone())
            }
            CachedEventContent::SystemMessage(msg) => {
                if let Some(text) = msg.text() {
                    tailwind_div!(text_color(theme.text.dim), items_center, flex).child(text)
                } else {
                    return None;
                }
            }
            CachedEventContent::UserMessage(msg) => {
                tailwind_div!().child(msg.body.clone().unwrap_or_default())
            }
            _ => tailwind_div!(),
        };

        Some(tailwind_div!().child(content))
    }
}
