use std::time::Duration;

use deplace_core::{formatting::fit_dimensions, state::MembershipMap};
use gpui::{
    Animation, AnimationExt, AnyElement, Div, Element, ElementId, InteractiveElement,
    LinearColorStop, ObjectFit, ParentElement, Pixels, Styled, StyledImage, div, img,
    linear_gradient, percentage, prelude::FluentBuilder, px, relative, transparent_black,
};
use gpui_component::{Colorize, StyledExt, red_600};
use macros::tailwind_div;
use matrix_sdk::{
    media::UniqueKey,
    ruma::{RoomId, UserId},
};

use crate::{
    components::{
        AvatarCache, CustomStyles, MemberRenderer,
        cache::{MediaState, ThumbnailCache},
        message::{
            CachedEventContent, CachedMessageType, CachedSendState, CachedSystemMessage,
            CachedTimelineEvent, CachedTimelineItem, CachedTimelineItemKind, CachedUserMessage,
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
        focused: bool,
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
                focused,
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
        focused: bool,
    ) -> AnyElement {
        let colors = &theme.colors;

        let show_highlight = self.flags.is_highlighted;

        let member = map.get(current_room_id).and_then(|m| m.get(&*self.sender));

        let sender_avatar =
            move |size: Pixels| member.render_avatar(size, size / 2.0, avatar_cache);
        let sender_name = move |size: Pixels| member.render_name(size);

        let small_icon_size = structure.chat.small_icon_size;

        let member_avatar = move |id: &UserId| {
            let member = map.get(current_room_id).and_then(|m| m.get(id));
            member.render_avatar(small_icon_size, small_icon_size / 2.0, avatar_cache)
        };

        let mut show_header = false;
        let mut pad_bottom = false;

        let content = match &self.content {
            CachedEventContent::FailedToParseMessageLike(text)
            | CachedEventContent::FailedToParseState(text) => {
                tailwind_div!(text_color(colors.error)).child(text.clone())
            }
            CachedEventContent::SystemMessage(msg) => {
                if let Some(div) = msg.render(
                    theme,
                    || sender_avatar(small_icon_size),
                    || sender_name(structure.chat.text_size),
                    member_avatar,
                ) {
                    div
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
                    if let CachedTimelineItemKind::Event(boxed_event) = &item.kind {
                        boxed_event.timestamp.abs_diff(self.timestamp) > 300
                            || boxed_event.sender != self.sender
                    } else {
                        true
                    }
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
        .when(focused, |el| el.border_color(colors.error))
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

impl CachedSystemMessage {
    fn render(
        &self,
        theme: &AppTheme,
        sender_avatar: impl Fn() -> AnyElement,
        sender_name: impl Fn() -> AnyElement,
        member_avatar: impl Fn(&UserId) -> AnyElement,
    ) -> Option<Div> {
        let parent = tailwind_div!(
            text_color(theme.text.dim),
            items_center,
            flex,
            flex_1,
            justify_center
        );
        match self {
            CachedSystemMessage::RtcNotification { text, declined_by } => {
                let declined_by_avatars = declined_by.iter().map(|id| member_avatar(id));

                Some(
                    parent
                        .child(sender_avatar())
                        .child(text.clone())
                        .children(declined_by_avatars),
                )
            }
            _ => self.text().map(|text| {
                parent
                    .child(sender_avatar())
                    .child(" ")
                    .child(sender_name())
                    .child(" ")
                    .child(text)
            }),
        }
    }
}

impl CachedUserMessage {
    fn render(&self, structure: &Structure, theme: &AppTheme, media_cache: &ThumbnailCache) -> Div {
        let warning = theme.colors.warning;
        let error = theme.colors.error;

        let chat = &structure.chat;

        let render_body = move |text| {
            tailwind_div!(text_color(theme.text.normal), flex, items_baseline)
                .child(text)
                .when(self.is_edited, |el| {
                    el.child(
                        tailwind_div!(
                            text_size(chat.small_text_size),
                            text_color(theme.text.muted)
                        )
                        .child(" (edited)"),
                    )
                })
                .into_any()
        };

        let content = match &self.msg_type {
            CachedMessageType::Empty => tailwind_div!(
                text_color(theme.text.muted),
                italic,
                text_size(chat.text_size)
            )
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
                border_color(theme.tile.border),
            )
            .child(render_icon(phosphor_svgs::icon::file::FILL, chat.icon_size))
            .child(
                tailwind_div!(flex, flex_col, gap(structure.small_gap), items_start)
                    .child(
                        tailwind_div!(text_color(theme.accent), hover(underline), cursor_pointer)
                            .id(filename.clone())
                            .child(filename.clone()),
                    )
                    .child(
                        tailwind_div!(
                            text_color(theme.text.muted),
                            text_size(chat.small_text_size),
                            group_hover(filename, |style| style.underline())
                        )
                        .child(
                            size.as_ref()
                                .map(|s| s.bytes_str.clone())
                                .unwrap_or("File".into()),
                        ),
                    ),
            )
            .into_any(),
            // TODO: Implement text based files
            CachedMessageType::Image {
                filename,
                source,
                source_key,
                width,
                height,
                size,
                format: _mime_type,
                blurhash_image,
            } => {
                let max_width = chat.max_media_width.as_f32();
                let max_height = chat.max_media_height.as_f32();

                let (width, height) = fit_dimensions(
                    width.unwrap_or(max_width),
                    height.unwrap_or(max_height),
                    max_width,
                    max_height,
                );

                let image = media_cache.get(source, source_key, width as u64, height as u64);

                const FADE_DURATION: Duration = Duration::from_millis(400);
                let loaded_elapsed =
                    media_cache.loaded_elapsed(&(source_key.clone(), width as u64, height as u64));

                let w = Pixels::from(width);
                let h = Pixels::from(height);

                let rounding = structure.inner_border_radius;

                let error_bg = theme.solid_bg.blend(error.alpha(0.05));
                let error_text_size = chat.text_size * 1.5;
                let error_fallback = move || {
                    tailwind_div!(
                        size_full,
                        bg(error_bg),
                        text_color(error)
                        outer_gradient(error, px(2.0)),
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
                    MediaState::Loading => {
                        if blurhash_image.is_some() {
                            div().size_full().into_any()
                        } else {
                            tailwind_div!(
                                size_full,
                                border_1,
                                bg(error),
                                border_color(theme.tile.border)
                            )
                            .child("dawdwdwadwa")
                            .into_any()
                        }
                    }
                    MediaState::Loaded(image) => {
                        let el = img(image)
                            .size_full()
                            .rounded(rounding)
                            .border_1()
                            .border_color(theme.tile.border)
                            .with_fallback(error_fallback);

                        match loaded_elapsed {
                            Some(elapsed) if elapsed < FADE_DURATION => {
                                let start_opacity =
                                    elapsed.as_secs_f32() / FADE_DURATION.as_secs_f32();
                                el.with_animation(
                                    ElementId::Name(filename.clone()),
                                    Animation::new(
                                        FADE_DURATION.checked_sub(elapsed).unwrap_or_default(),
                                    ),
                                    move |img, delta| {
                                        img.opacity(start_opacity + (1.0 - start_opacity) * delta)
                                    },
                                )
                                .into_any()
                            }
                            _ => el.into_any(),
                        }
                    }
                    MediaState::Failed => error_fallback().into_any(),
                };

                let blurhash_overlay = blurhash_image.clone().and_then(|blurhash| {
                    let el = img(blurhash)
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .rounded(rounding);

                    match loaded_elapsed {
                        None => Some(el.into_any()),
                        Some(elapsed) if elapsed < FADE_DURATION => {
                            let start_opacity =
                                1.0 - elapsed.as_secs_f32() / FADE_DURATION.as_secs_f32();
                            Some(
                                el.with_animation(
                                    ElementId::Name(format!("{filename}-blurhash-fade-out").into()),
                                    Animation::new(
                                        FADE_DURATION.checked_sub(elapsed).unwrap_or_default(),
                                    ),
                                    move |el, delta| el.opacity(start_opacity * (1.0 - delta)),
                                )
                                .into_any(),
                            )
                        }
                        Some(_) => None,
                    }
                });

                tailwind_div!(flex, flex_col, gap(structure.small_gap))
                    .when_some(self.body.as_ref(), |el, text| {
                        el.child(render_body(text.clone()))
                    })
                    .child(
                        tailwind_div!(w(w), h(h), rounded(structure.inner_border_radius))
                            .group(filename)
                            .relative()
                            .when_some(blurhash_overlay, |el, overlay| el.child(overlay))
                            .child(content)
                            .child(
                                tailwind_div!(
                                    absolute,
                                    bottom(structure.small_gap),
                                    left(structure.small_gap),
                                    paddings(structure.small_gap),
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
            // TODO: Live locations are not supported yet
            CachedMessageType::LiveLocation { .. } => tailwind_div!(text_color(warning))
                .child("Live locations are not supported yet")
                .into_any(),
            // TODO: Locations are not supported yet
            CachedMessageType::Location(_) => tailwind_div!(text_color(warning))
                .child("Locations are not supported yet")
                .into_any(),
            // TODO: Notices are not supported yet
            CachedMessageType::Notice => tailwind_div!(text_color(warning))
                .child("Notices are not supported yet")
                .into_any(),
            CachedMessageType::Other { msg_type } => tailwind_div!(text_color(warning))
                .child("Unsupported message type: ")
                .child(msg_type.clone())
                .into_any(),
            // TODO: Polls are not supported yet
            CachedMessageType::Poll => tailwind_div!(text_color(warning))
                .child("Polls are not supported yet")
                .into_any(),
            CachedMessageType::ServerNotice { admin_contact } => {
                tailwind_div!(text_color(theme.text.normal))
                    .child("Server notice")
                    .when_some(admin_contact.clone(), |el, text| {
                        el.child(", contact: ").child(text.clone())
                    })
                    .into_any()
            }
            // TODO: Stickers are not supported yet
            CachedMessageType::Sticker => tailwind_div!(text_color(theme.text.normal))
                .child("Stickers are not supported yet")
                .into_any(),
            CachedMessageType::Text => {
                if let Some(text) = self.body.clone() {
                    render_body(text)
                } else {
                    tailwind_div!(text_color(theme.text.dim), italic)
                        .child("Empty message")
                        .into_any()
                }
            }
            CachedMessageType::UnableToDecrypt => tailwind_div!(text_color(error))
                .child("Unable to decrypt message")
                .into_any(),
            _ => tailwind_div!(text_color(theme.text.normal)).into_any(),
        };

        tailwind_div!(line_height(relative(1.0)), flex, flex_col)
            .when_some(self.in_reply_to.clone(), |el, reply| {
                match reply {
                    _ => {}
                };
                el
            })
            .child(content)
    }
}
