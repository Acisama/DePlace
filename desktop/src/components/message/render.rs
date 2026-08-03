use deplace_core::{formatting::fit_dimensions, state::MembershipMap};
use gpui::{
    AnyElement, Div, Element, ElementId, InteractiveElement, LinearColorStop, ParentElement,
    Pixels, Styled, StyledImage, div, img, linear_gradient, percentage, prelude::FluentBuilder,
    relative, transparent_black,
};
use gpui_component::{Colorize, StyledExt, red_600};
use macros::tailwind_div;
use matrix_sdk::ruma::RoomId;

use crate::{
    components::{
        AvatarCache, CustomStyles, MemberRenderer,
        cache::{MediaState, ThumbnailCache},
        message::{
            CachedEventContent, CachedMessageType, CachedSendState, CachedTimelineEvent,
            CachedTimelineItem, CachedTimelineItemKind, CachedUserMessage,
        },
        render_icon,
    },
    theme::{AppTheme, Structure},
};

#[allow(clippy::too_many_arguments)]
impl CachedTimelineItem {
    pub fn render(
        &self,
        prev: Option<&Self>,
        next: Option<&Self>,
        theme: &AppTheme,
        structure: &Structure,
        curent_room_id: &RoomId,
        map: &MembershipMap,
        avatar_cache: &AvatarCache,
        image_cache: &ThumbnailCache,
    ) -> AnyElement {
        let divider_width = structure.divider_width;

        let content = match &self.kind {
            // TODO: Merge with read marker if adjacent
            // fn is_read_marker(item: Option<Arc<TimelineItem>>) -> bool {
            //     item.and_then(|i| i.as_virtual().cloned())
            //         .is_some_and(|v| matches!(v, VirtualTimelineItem::ReadMarker))
            // }
            CachedTimelineItemKind::DateDivider(date) => {
                tailwind_div!(w_full, flex, items_center, gap(structure.gap))
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
                structure,
                curent_room_id,
                map,
                avatar_cache,
                image_cache,
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
        structure: &Structure,
        current_room_id: &RoomId,
        map: &MembershipMap,
        avatar_cache: &AvatarCache,
        image_cache: &ThumbnailCache,
        prev: Option<&CachedTimelineItem>,
        next: Option<&CachedTimelineItem>,
    ) -> AnyElement {
        let colors = &theme.colors;

        let show_highlight = self.flags.is_highlighted;

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
                    tailwind_div!(
                        text_color(theme.text.dim),
                        items_center,
                        flex,
                        flex_1,
                        justify_center
                    )
                    .child(sender_avatar(structure.chat.small_icon_size))
                    .child(" ")
                    .child(sender_name(structure.chat.text_size))
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
                            if let CachedTimelineItemKind::Event(boxed_event) = &item.kind {
                                if let CachedEventContent::UserMessage(_) = &boxed_event.content {
                                    boxed_event.timestamp.abs_diff(self.timestamp) > 300
                                        || boxed_event.sender != self.sender
                                } else {
                                    true
                                }
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
                msg.render(structure, theme, image_cache)
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

        let icon_size = structure.chat.icon_size;
        let col_width = icon_size + 2.0 * structure.gap;

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
            structure.small_gap + Pixels::from(2.0)
        } else {
            Pixels::ZERO
        };
        let mb = if pad_bottom {
            structure.small_gap + Pixels::from(2.0)
        } else {
            Pixels::ZERO
        };

        tailwind_div!(
            w_full,
            border_transparent,
            rounded(structure.small_gap),
            group("message"),
            bg(bg),
            hover(border_color(theme.tile.border), bg(hover_bg)),
            flex,
            py(structure.small_gap),
            mt(mt),
            mb(mb),
            flex_row,
            text_color(text_color),
            text_size(structure.chat.text_size),
        )
        .when(self.flags.contains_only_emojis, |el| {
            el.text_size(structure.chat.text_size * 2.0)
        })
        .id(id.clone())
        .child(
            tailwind_div!(w(col_width), px(structure.gap))
                .when(show_header, |el| {
                    el.child(sender_avatar(structure.chat.icon_size))
                })
                .when(!show_header, |el| {
                    el.child(
                        tailwind_div!(
                            text_color(transparent_black()),
                            text_size(structure.chat.small_text_size),
                            font_semibold
                        )
                        .id(id)
                        .group_hover("message", |style| style.text_color(theme.text.muted))
                        .child(self.short_time.clone()),
                    )
                }),
        )
        .child(
            tailwind_div!(flex, size_full, flex_col, gap(structure.small_gap))
                .when(show_header, |el| {
                    el.child(
                        tailwind_div!(flex, flex_row, gap(structure.gap))
                            .child(sender_name(structure.chat.text_size))
                            .child(
                                tailwind_div!(
                                    text_size(structure.chat.small_text_size),
                                    text_color(theme.text.muted),
                                    font_semibold
                                )
                                .child(self.long_time.clone()),
                            ),
                    )
                })
                .child(content),
        )
        .into_any()
    }
}

impl CachedUserMessage {
    pub fn render(
        &self,
        structure: &Structure,
        theme: &AppTheme,
        media_cache: &ThumbnailCache,
    ) -> Div {
        let warning = theme.colors.warning;
        let error = theme.colors.error;

        let chat = &structure.chat;

        let content = match &self.msg_type {
            CachedMessageType::Empty => tailwind_div!(text_color(theme.text.dim), italic)
                .child("Empty message")
                .into_any(),
            CachedMessageType::Redacted => tailwind_div!(text_color(theme.text.dim), italic)
                .child("Message redacted")
                .into_any(),
            CachedMessageType::Emote => tailwind_div!(text_color(theme.text.normal))
                .child("Emote")
                .into_any(),
            // TODO: Audio messages are not supported yet
            CachedMessageType::Audio { .. } => tailwind_div!(text_color(warning))
                .child("Audio messages are not supported yet")
                .into_any(),
            // TODO: Emit notification if user clicks on file
            CachedMessageType::File { filename, size, .. } => tailwind_div!(
                bg(theme.solid_bg),
                flex,
                flex_row,
                items_start,
                w(relative(0.3))
                gap(structure.small_gap),
                paddings(structure.gap),
                rounded(structure.inner_border_radius),
                border_1,
                border_color(theme.tile.border)
            )
            .child(render_icon(phosphor_svgs::icon::file::FILL, chat.icon_size))
            .child(
                tailwind_div!(flex, flex_col, gap(structure.small_gap), items_start)
                    .child(
                        tailwind_div!(text_color(theme.accent), hover(underline))
                            .child(filename.clone()),
                    )
                    .child(
                        tailwind_div!(
                            text_color(theme.text.muted),
                            text_size(chat.small_text_size)
                        )
                        .child(
                            size.as_ref()
                                .map(|s| s.bytes_str.clone())
                                .unwrap_or("File".into()),
                        ),
                    ),
            )
            .into_any(),
            CachedMessageType::Image {
                filename,
                source,
                width,
                height,
                size,
                mime_type,
                blurhash_image,
            } => {
                let max_width = chat.max_media_width.as_f32();
                let max_height = chat.max_media_height.as_f32();

                let (w, h) = fit_dimensions(
                    width.unwrap_or(max_width),
                    height.unwrap_or(max_height),
                    max_width,
                    max_height,
                );

                let image = media_cache.get(source, w as u64, h as u64);

                let w = Pixels::from(w);
                let h = Pixels::from(h);

                let rounding = structure.inner_border_radius;

                let error_bg = theme.solid_bg.blend(error.alpha(0.05));
                let error_text_size = chat.text_size * 1.5;
                let error_fallback = move || {
                    tailwind_div!(
                        size_full,
                        bg(error_bg),
                        text_color(error)
                        outer_gradient(error),
                        flex,
                        items_center,
                        justify_center,
                        text_size(error_text_size),
                        text_center,
                        rounded(rounding)
                    )
                    .child("Failed to load")
                    .into_any()
                };

                let content = match image {
                    MediaState::Loading => match blurhash_image {
                        Some(image) => img(image.clone())
                            .border_1()
                            .border_color(theme.tile.border)
                            .size_full()
                            .rounded(rounding)
                            .into_any(),
                        None => tailwind_div!(
                            size_full,
                            border_1,
                            bg(error),
                            border_color(theme.tile.border)
                        )
                        .child("dawdwdwadwa")
                        .into_any(),
                    },
                    MediaState::Loaded(image) => img(image)
                        .size_full()
                        .rounded(rounding)
                        .border_1()
                        .border_color(theme.tile.border)
                        .with_fallback(error_fallback)
                        .into_any(),
                    MediaState::Failed => error_fallback().into_any(),
                };

                tailwind_div!(flex, flex_col, gap(structure.small_gap))
                    .when_some(self.body.as_ref(), |el, text| {
                        el.child(div().text_color(theme.text.normal))
                            .child(text.clone())
                    })
                    .child(
                        tailwind_div!(
                            w(w),
                            h(h),
                            bg(theme.solid_bg),
                            rounded(structure.inner_border_radius)
                        )
                        .group(filename)
                        .relative()
                        .child(content)
                        .child(
                            tailwind_div!(
                                absolute,
                                bottom(structure.small_gap),
                                left(structure.small_gap),
                                paddings(structure.small_gap),
                                bg(theme.solid_bg),
                                rounded(structure.smaller_border_radius)
                                border_1,
                                border_color(theme.tile.border),
                                opacity(0.0),
                                flex,
                                items_center,
                            )
                            .group_hover(filename, |style| style.opacity(1.0))
                            .child(filename.clone())
                            .when_some(size.clone(), |el, size| {
                                el.child(" (").child(size.bytes_str).child(")")
                            }),
                        ),
                    )
                    .into_any()
            }
            _ => tailwind_div!(text_color(theme.text.normal)).into_any(),
        };

        tailwind_div!(line_height(relative(1.0))).child(content)
    }
}
