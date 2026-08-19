use std::{ops::Range, sync::Arc, time::Duration};

use deplace_core::{
    NameExt,
    colors::ColorExt,
    formatting::fit_dimensions,
    settings::Settings,
    state::{AppState, MembershipMap},
};
use gpui::{
    Animation, AnimationExt, AnyElement, Div, Element, ElementId, FontStyle, FontWeight,
    HighlightStyle, Hsla, InteractiveElement, InteractiveText, IntoElement, LinearColorStop,
    ObjectFit, ParentElement, Pixels, SharedString, StatefulInteractiveElement, StrikethroughStyle,
    Styled, StyledImage, StyledText, TextStyle, UnderlineStyle, Window, div, img, linear_gradient,
    prelude::FluentBuilder, px, relative, transparent_black,
};
use gpui_component::{StyledExt, red_600};
use macros::tailwind_div;
use matrix_sdk::ruma::{OwnedEventId, RoomId, UserId};

use crate::{
    cache::{AvatarCache, MediaState, ThumbnailCache},
    components::{
        CustomStyles,
        message::{
            CachedEventContent, CachedMessageType, CachedReplyInfo, CachedSendState,
            CachedSystemMessage, CachedTimelineEvent, CachedTimelineItem, CachedTimelineItemKind,
            CachedUserMessage, DetailState, ReactionInfo,
            text::{CachedBlock, CachedLink, CachedPill, CachedRichText},
        },
        profiles::{MemberRenderer, render_icon},
    },
    saving::save_file,
    things::{AppTheme, ImportantPaths, Structure},
};

#[allow(clippy::too_many_arguments)]
impl CachedTimelineItem {
    pub fn render(
        &self,
        window: &Window,
        theme: &AppTheme,
        structure: &Structure,
        curent_room_id: &RoomId,
        map: &MembershipMap,
        avatar_cache: &AvatarCache,
        image_cache: &ThumbnailCache,
        focused: bool,
        on_toggle_reaction: impl Fn(Arc<OwnedEventId>, SharedString) + Clone + 'static,
        state: &AppState,
        importantpaths: &ImportantPaths,
    ) -> AnyElement {
        let divider_width = structure.divider_width;

        match &self.kind {
            // TODO: Merge with read marker if adjacent
            // fn is_read_marker(item: Option<Arc<TimelineItem>>) -> bool {
            //     item.and_then(|i| i.as_virtual().cloned())
            //         .is_some_and(|v| matches!(v, VirtualTimelineItem::ReadMarker))
            // }
            CachedTimelineItemKind::DateDivider {
                date,
                depends_on_system_messages,
            } => {
                if let Some(types) = depends_on_system_messages
                    && state
                        .settings()
                        .system_messages_to_show
                        .value()
                        .is_disjoint(*types)
                {
                    return div().into_any();
                }

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
                window,
                theme,
                structure,
                curent_room_id,
                map,
                avatar_cache,
                image_cache,
                focused,
                on_toggle_reaction,
                state,
                importantpaths,
            ),
        }
    }
}

#[allow(clippy::too_many_arguments)]
impl CachedTimelineEvent {
    fn render(
        &self,
        id: ElementId,
        window: &Window,
        theme: &AppTheme,
        structure: &Structure,
        current_room_id: &RoomId,
        map: &MembershipMap,
        avatar_cache: &AvatarCache,
        image_cache: &ThumbnailCache,
        focused: bool,
        on_toggle_reaction: impl Fn(Arc<OwnedEventId>, SharedString) + Clone + 'static,
        state: &AppState,
        importantpaths: &ImportantPaths,
    ) -> AnyElement {
        let colors = &theme.colors;

        let event_id = self.event_id.clone();
        let toggle_reaction = move |reaction: SharedString| {
            if let Some(event_id) = event_id.clone() {
                on_toggle_reaction(event_id, reaction);
            }
        };

        let show_highlight = self.flags.is_highlighted;

        let member = map.get(current_room_id).and_then(|m| m.get(&*self.sender));

        let sender_avatar =
            move |size: Pixels| member.render_avatar(size, size / 2.0, theme, avatar_cache, None);
        let sender_name = move |size: Pixels| member.render_name(size, &theme.colors);

        let small_icon_size = structure.chat.small_icon_size;

        let member_avatar = move |id: &UserId| {
            let member = map.get(current_room_id).and_then(|m| m.get(id));
            member.render_avatar(
                small_icon_size,
                small_icon_size / 2.0,
                theme,
                avatar_cache,
                None,
            )
        };

        let smaller_member_avatar = move |id: &UserId| {
            let member = map.get(current_room_id).and_then(|m| m.get(id));
            member.render_avatar(
                structure.chat.small_text_size * 1.2,
                structure.chat.small_text_size * 1.2 / 2.0,
                theme,
                avatar_cache,
                None,
            )
        };

        let member_name = move |id: &UserId| {
            let member = map.get(current_room_id).and_then(|m| m.get(id));
            member.render_name(structure.chat.small_text_size, colors)
        };

        // Same lookup as `member_avatar`/`member_name` above, but returning the plain
        // name text + tint color instead of a built element - what mention pills inside
        // a formatted message body need to build a `TextRun`.
        let member_name_color = move |id: &UserId| -> (SharedString, gpui::Hsla) {
            match map.get(current_room_id).and_then(|m| m.get(id)) {
                Some(member) => (member.get_name().into(), member.color().into()),
                None => ("Unknown".into(), colors.unknown),
            }
        };

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
                    state.settings(),
                ) {
                    div
                } else {
                    return div().into_any();
                }
            }
            CachedEventContent::UserMessage(msg) => msg.render(
                &id,
                window,
                structure,
                theme,
                image_cache,
                member_avatar,
                member_name_color,
                toggle_reaction,
                state,
                importantpaths,
            ),
        };

        let highlight_color = if show_highlight {
            Some(theme.accent)
        } else {
            None
        };

        let (bg, hover_bg) = if let Some(color) = highlight_color {
            let color = color.alpha(0.1);
            (
                linear_gradient(
                    90.0,
                    LinearColorStop {
                        color,
                        percentage: 0.5,
                    },
                    LinearColorStop {
                        color: transparent_black(),
                        percentage: 1.1,
                    },
                ),
                linear_gradient(
                    90.0,
                    LinearColorStop {
                        color,
                        percentage: 0.5,
                    },
                    LinearColorStop {
                        color: theme.tile.background,
                        percentage: 1.1,
                    },
                ),
            )
        } else {
            (transparent_black().into(), theme.tile.background.into())
        };

        let text_color = self
            .state
            .as_ref()
            .map(|s| match s {
                CachedSendState::NotSentYet { .. } => theme.text.dim,
                CachedSendState::SendingFailed { .. } => colors.error,
                CachedSendState::Sent { .. } => theme.text.normal,
            })
            .unwrap_or(theme.text.normal);

        let mt = if self.show_header {
            structure.small_gap * 2.0
        } else {
            Pixels::ZERO
        };
        let mb = if self.pad_bottom {
            structure.small_gap / 2.0
        } else {
            Pixels::ZERO
        };

        let pre_col_space = structure.small_gap * 1.5;

        let is_system_message = matches!(self.content, CachedEventContent::SystemMessage(_));

        // check whether the message is short enough to
        // decide where to display the read marker
        let is_short_message = !self.show_header
            && self.in_reply_to().is_none()
            && matches!(&self.content, CachedEventContent::UserMessage(msg) if {
                msg.reactions.as_ref().is_none_or(|r| r.is_empty())
                    && match &msg.msg_type {
                        CachedMessageType::Text | CachedMessageType::Emote => {
                            match &msg.body {
                                Some(blocks) if blocks.len() == 1 => match &blocks[0] {
                                    CachedBlock::Paragraph(rich_text) => {
                                        !rich_text.text.contains('\n')
                                        // && rich_text.text.chars().count() <= 60
                                        // TODO: Implement when wrapping is added
                                    }
                                    _ => false,
                                },
                                None => true,
                                _ => false,
                            }
                        }
                        _ => false,
                    }
            });

        let outer = tailwind_div!(
            w_full,
            border_transparent,
            rounded(pre_col_space / 2.0),
            group("message"),
            bg(bg),
            hover(border_color(theme.tile.border), bg(hover_bg)),
            flex,
            flex_col,
            text_color(text_color),
            text_size(structure.chat.text_size),
        )
        .when(focused, |el| el.border_color(colors.error))
        .when(self.flags.contains_only_emojis, |el| {
            el.text_size(structure.chat.text_size * 2.0)
        })
        .id(id.clone())
        .when_some(self.in_reply_to(), |el, reply| {
            el.child(reply.render(theme, structure, smaller_member_avatar, member_name))
        })
        .when(!self.read_by.is_empty(), |el| {
            el.child(
                tailwind_div!(
                    absolute,
                    right(structure.small_gap),
                    flex,
                    flex_row,
                    gap(structure.small_gap),
                    items_center,
                    cursor_pointer
                )
                .when_else(
                    is_short_message,
                    |style| style.top(Pixels::ZERO).bottom(Pixels::ZERO),
                    |style| style.bottom(structure.small_gap),
                )
                .children(self.read_by.iter().map(|id| member_avatar(id))),
            )
        });

        let body = if is_system_message {
            outer.child(content).into_any_element()
        } else {
            outer
                .child(
                    tailwind_div!(flex, flex_row)
                        .child(
                            tailwind_div!(
                                w(structure.chat_col_width()),
                                px(pre_col_space),
                                relative
                            )
                            .when_else(
                                self.show_header,
                                |el| el.child(sender_avatar(structure.chat.icon_size)),
                                |el| {
                                    el.child(
                                        tailwind_div!(
                                            text_color(transparent_black()),
                                            text_size(structure.chat.small_text_size),
                                        )
                                        .when(is_short_message, |el| {
                                            el.flex()
                                                .justify_center()
                                                .text_center()
                                                .h_full()
                                                .items_center()
                                        })
                                        .id(id)
                                        .group_hover("message", |style| {
                                            style.text_color(theme.text.muted)
                                        })
                                        .child(self.short_time.clone()),
                                    )
                                },
                            ),
                        )
                        .child(
                            tailwind_div!(flex, size_full, flex_col)
                                .when(self.show_header, |el| {
                                    el.child(
                                        tailwind_div!(flex, flex_row)
                                            .child(sender_name(structure.chat.text_size))
                                            .child(" ")
                                            .child(
                                                tailwind_div!(
                                                    text_size(structure.chat.small_text_size),
                                                    text_color(theme.text.muted),
                                                )
                                                .child(self.long_time.clone()),
                                            ),
                                    )
                                })
                                .child(content),
                        ),
                )
                .into_any_element()
        };

        tailwind_div!(w_full, pt(mt), pb(mb)).child(body).into_any()
    }
}

impl CachedSystemMessage {
    fn render(
        &self,
        theme: &AppTheme,
        sender_avatar: impl Fn() -> AnyElement,
        sender_name: impl Fn() -> AnyElement,
        member_avatar: impl Fn(&UserId) -> AnyElement,
        settings: Settings,
    ) -> Option<Div> {
        let message_to_show = settings.system_messages_to_show.value();

        if !self.should_show(message_to_show) {
            return None;
        }

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
                        .child(" ")
                        .child(sender_name())
                        .child(" ")
                        .child(text.clone())
                        .child(" ")
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
    #[allow(clippy::too_many_arguments)]
    fn render(
        &self,
        id: &ElementId,
        window: &Window,
        structure: &Structure,
        theme: &AppTheme,
        media_cache: &ThumbnailCache,
        member_avatar: impl Fn(&UserId) -> AnyElement,
        member_name_color: impl Fn(&UserId) -> (SharedString, Hsla),
        on_toggle_reaction: impl Fn(SharedString) + Clone + 'static,
        state: &AppState,
        importantpaths: &ImportantPaths,
    ) -> Div {
        let warning = theme.colors.warning;
        let error = theme.colors.error;

        let chat = &structure.chat;
        let id_prefix: SharedString = format!("{id:?}").into();
        let base_text_style = window.text_style();

        let data_size_unit = &state.settings().data_size_unit.value();

        let text_style_clone = base_text_style.clone();
        let render_body = move |blocks: Arc<[CachedBlock]>| {
            tailwind_div!(text_color(theme.text.normal), flex, items_baseline)
                .child(render_rich_body(
                    &id_prefix,
                    &blocks,
                    &text_style_clone,
                    theme,
                    structure,
                    &member_name_color,
                ))
                .when(self.is_edited, |el| {
                    el.child(
                        tailwind_div!(
                            text_size(chat.small_text_size),
                            text_color(theme.text.muted)
                        )
                        .child(" (edited)"),
                    )
                })
                .cursor_text()
                .into_any()
        };

        let content = match &self.msg_type {
            CachedMessageType::Empty => tailwind_div!(
                text_color(theme.text.muted),
                italic,
                text_size(chat.text_size)
            )
            .child("Empty message")
            .cursor_text()
            .into_any(),
            CachedMessageType::Redacted => tailwind_div!(text_color(theme.text.dim), italic)
                .child("Message redacted")
                .cursor_text()
                .into_any(),
            CachedMessageType::Emote => {
                if let Some(text) = self.body.clone() {
                    render_body(text)
                } else {
                    tailwind_div!(
                        text_color(theme.text.dim),
                        text_size(chat.text_size * 1.5),
                        italic
                    )
                    .cursor_text()
                    .child("Empty message")
                    .into_any()
                }
            }
            // TODO: Audio messages are not supported yet
            CachedMessageType::Audio { .. } => tailwind_div!(text_color(warning))
                .child("Audio messages are not supported yet")
                .cursor_text()
                .into_any(),
            // TODO: Emit notification if user clicks on file
            CachedMessageType::File {
                filename,
                size,
                source,
                ..
            } => tailwind_div!(
                bg(theme.solid_bg),
                flex,
                flex_shrink_1,
                flex_row,
                items_start,
                self_start,
                min_w(relative(0.3)),
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
                            .child(filename.clone())
                            .on_click({
                                let client = state.client();
                                let source = source.clone();
                                let filename = filename.clone();
                                let importantpaths = importantpaths.clone();

                                move |_, _, cx| {
                                    save_file(
                                        cx,
                                        client.clone(),
                                        &source,
                                        &filename,
                                        &importantpaths,
                                    )
                                }
                            }),
                    )
                    .child(
                        tailwind_div!(
                            text_color(theme.text.muted),
                            text_size(chat.small_text_size),
                            group_hover(filename, |style| style.underline())
                        )
                        .child(
                            size.as_ref()
                                .map(|s| s.get(data_size_unit))
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

                let label_text = match size {
                    Some(size) => format!("{filename} ({})", size.get(data_size_unit)),
                    None => filename.to_string(),
                };
                let label_run = base_text_style.clone().to_run(label_text.len());
                let label_text_width = window
                    .text_system()
                    .layout_line(&label_text, chat.text_size, &[label_run], None)
                    .width;

                let min_width = (label_text_width + structure.small_gap * 4.0).as_f32();

                let (width, height) = fit_dimensions(
                    width.unwrap_or(max_width),
                    height.unwrap_or(max_height),
                    max_width,
                    max_height,
                    min_width,
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
                                    rounded(
                                        (structure.smaller_border_radius
                                            + structure.inner_border_radius)
                                            / 2.0
                                    ),
                                    border_1,
                                    border_color(theme.tile.border),
                                    opacity(0.0),
                                    bg(theme.solid_bg),
                                    flex,
                                    items_center,
                                )
                                .group_hover(filename, |style| style.opacity(1.0))
                                .child(filename.clone())
                                .when_some(
                                    size.clone(),
                                    |el, size| {
                                        el.child(" (").child(size.get(data_size_unit)).child(")")
                                    },
                                ),
                            ),
                    )
                    .into_any()
            }
            // TODO: Live locations are not supported yet
            CachedMessageType::LiveLocation { .. } => tailwind_div!(text_color(warning))
                .child("Live locations are not supported yet")
                .cursor_text()
                .into_any(),
            // TODO: Locations are not supported yet
            CachedMessageType::Location(_) => tailwind_div!(text_color(warning))
                .child("Locations are not supported yet")
                .cursor_text()
                .into_any(),
            // TODO: Notices are not supported yet
            CachedMessageType::Notice => tailwind_div!(text_color(warning))
                .child("Notices are not supported yet")
                .cursor_text()
                .into_any(),
            CachedMessageType::Other { msg_type } => tailwind_div!(text_color(warning))
                .child("Unsupported message type: ")
                .child(msg_type.clone())
                .cursor_text()
                .into_any(),
            // TODO: Polls are not supported yet
            CachedMessageType::Poll => tailwind_div!(text_color(warning))
                .child("Polls are not supported yet")
                .cursor_text()
                .into_any(),
            CachedMessageType::ServerNotice { admin_contact } => {
                tailwind_div!(text_color(theme.text.normal))
                    .child("Server notice")
                    .cursor_text()
                    .when_some(admin_contact.clone(), |el, text| {
                        el.child(", contact: ").child(text.clone())
                    })
                    .into_any()
            }
            // TODO: Stickers are not supported yet
            CachedMessageType::Sticker => tailwind_div!(text_color(theme.text.normal))
                .child("Stickers are not supported yet")
                .cursor_text()
                .into_any(),
            CachedMessageType::Text => {
                if let Some(text) = self.body.clone() {
                    render_body(text)
                } else {
                    tailwind_div!(text_color(theme.text.dim), italic)
                        .child("Empty message")
                        .cursor_text()
                        .into_any()
                }
            }
            CachedMessageType::UnableToDecrypt => tailwind_div!(text_color(error))
                .child("Unable to decrypt message")
                .cursor_text()
                .into_any(),
            _ => tailwind_div!(text_color(theme.text.normal)).into_any(),
        };

        tailwind_div!(line_height(relative(1.0)), text_center, flex, flex_col)
            .child(content)
            .when_some(self.reactions.clone(), |el, reactions| {
                el.child(render_reactions(
                    reactions,
                    theme,
                    structure,
                    member_avatar,
                    on_toggle_reaction,
                ))
            })
    }
}

fn render_reactions(
    reactions: Vec<ReactionInfo>,
    theme: &AppTheme,
    structure: &Structure,
    member_avatar: impl Fn(&UserId) -> AnyElement,
    on_toggle_reaction: impl Fn(SharedString) + Clone + 'static,
) -> Div {
    let children = reactions.into_iter().map(|info| {
        let reactors = info.reactors;
        let emoji = info.emoji.clone();
        let on_toggle_reaction = on_toggle_reaction.clone();

        tailwind_div!(
            flex,
            flex_row,
            gap(structure.small_gap),
            paddings(structure.small_gap),
            border(structure.divider_width),
            border_color(theme.solid_hover_bg),
            items_center,
            bg(theme.solid_bg),
            rounded(structure.inner_border_radius),
            cursor_pointer
        )
        .id(info.emoji.clone())
        .on_click(move |_, _, _| on_toggle_reaction(emoji.clone()))
        .when(info.has_own, |el| {
            el.border_color(theme.accent).bg(theme.accent_bg())
        })
        .child(info.emoji)
        .child(info.reactors_count)
        .child(
            tailwind_div!(flex, flex_row_reverse, gap(-structure.gap))
                .children(reactors.iter().map(|id| member_avatar(id))),
        )
    });

    tailwind_div!(
        flex,
        flex_row,
        gap(structure.small_gap),
        mt(structure.small_gap)
    )
    .children(children)
}

impl CachedReplyInfo {
    fn render(
        &self,
        theme: &AppTheme,
        structure: &Structure,
        sender_avatar: impl Fn(&UserId) -> AnyElement,
        sender_name: impl Fn(&UserId) -> AnyElement,
    ) -> Div {
        let content = match &self.body {
            DetailState::Pending => {
                tailwind_div!(italic, text_color(theme.text.dim)).child("Loading")
            }
            DetailState::Error(error) => {
                tailwind_div!(text_color(theme.colors.error)).child(error.clone())
            }
            DetailState::Unavailable => {
                tailwind_div!(text_color(theme.colors.warning)).child("Unavailable")
            }
            DetailState::Ready(content) => {
                let (icon, text, style) = content.body.render_things(theme, structure);

                tailwind_div!(flex, flex_row, items_center, text_center)
                    .child(sender_avatar(&content.sender_id))
                    .child(" ")
                    .child(sender_name(&content.sender_id))
                    .child(" ")
                    .when_some(icon, |el, icon| {
                        el.child(render_icon(icon, structure.chat.small_text_size))
                    })
                    .when_some(text, |el, text| {
                        el.child(
                            tailwind_div!(
                                text_color(theme.text.normal),
                                text_size(structure.chat.small_text_size),
                                line_height(relative(1.0))
                            )
                            .child(text)
                            .refine_style(&style),
                        )
                    })
            }
        };

        let col_width = structure.chat_col_width();

        tailwind_div!(
            flex,
            flex_row,
            cursor_pointer,
            pb(structure.small_gap / 2.0)
        )
        .child(
            tailwind_div!(w(col_width), mb(structure.small_gap), relative).child(tailwind_div!(
                absolute,
                left(col_width / 2.0),
                bottom(-structure.small_gap),
                w(col_width / 2.0 - structure.small_gap / 2.0),
                h((structure.chat.small_text_size * 1.2 + structure.small_gap * 1.5) / 2.0),
                rounded_tl(structure.outer_border_radius),
                border_l(structure.divider_width),
                border_t(structure.divider_width),
                border_color(theme.tile.border)
            )),
        )
        .child(content)
    }
}

pub(crate) fn render_rich_body(
    id: &SharedString,
    blocks: &[CachedBlock],
    base_text_style: &TextStyle,
    theme: &AppTheme,
    structure: &Structure,
    member_name: &dyn Fn(&UserId) -> (SharedString, Hsla),
) -> AnyElement {
    let mut counter = 0;
    render_blocks(
        id,
        blocks,
        base_text_style,
        theme,
        structure,
        member_name,
        &mut counter,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_blocks(
    id: &SharedString,
    blocks: &[CachedBlock],
    base_text_style: &TextStyle,
    theme: &AppTheme,
    structure: &Structure,
    member_name: &dyn Fn(&UserId) -> (SharedString, Hsla),
    counter: &mut usize,
) -> AnyElement {
    tailwind_div!(flex, flex_col, gap(structure.small_gap))
        .children(blocks.iter().map(|block| {
            render_block(
                id,
                block,
                base_text_style,
                theme,
                structure,
                member_name,
                counter,
            )
        }))
        .into_any()
}

fn next_id(id: &SharedString, counter: &mut usize) -> SharedString {
    *counter += 1;
    format!("{id}-rich-{counter}").into()
}

#[allow(clippy::too_many_arguments)]
fn render_block(
    id: &SharedString,
    block: &CachedBlock,
    base_text_style: &TextStyle,
    theme: &AppTheme,
    structure: &Structure,
    member_name: &dyn Fn(&UserId) -> (SharedString, Hsla),
    counter: &mut usize,
) -> AnyElement {
    match block {
        CachedBlock::Paragraph(text) => {
            render_rich_text(id, text, base_text_style, theme, member_name, counter)
        }
        CachedBlock::Heading { level, text } => {
            let scale = 1.6 - 0.1 * f32::from((*level).min(6));
            tailwind_div!(font_bold, text_size(structure.chat.text_size * scale))
                .child(render_rich_text(
                    id,
                    text,
                    base_text_style,
                    theme,
                    member_name,
                    counter,
                ))
                .into_any()
        }
        CachedBlock::List {
            ordered,
            start,
            items,
        } => {
            let base = start.unwrap_or(1);
            tailwind_div!(
                flex,
                flex_col,
                gap(structure.small_gap),
                pl(structure.small_gap)
            )
            .children(items.iter().enumerate().map(|(ix, item)| {
                let marker: SharedString = if *ordered {
                    format!("{}.", base + ix as i64).into()
                } else {
                    "•".into()
                };
                tailwind_div!(flex, flex_row, gap(structure.small_gap))
                    .child(div().child(marker))
                    .child(render_blocks(
                        id,
                        item,
                        base_text_style,
                        theme,
                        structure,
                        member_name,
                        counter,
                    ))
                    .into_any()
            }))
            .into_any()
        }
        CachedBlock::CodeBlock { code, language: _ } => tailwind_div!(
            bg(theme.solid_bg),
            rounded(structure.inner_border_radius),
            border_1,
            border_color(theme.tile.border),
            paddings(structure.gap),
            font_family("monospace"),
            text_size(structure.chat.small_text_size)
        )
        .child(code.clone())
        .into_any(),
        CachedBlock::Quote(children) => tailwind_div!(
            flex,
            pl(structure.gap),
            border_l(structure.divider_width * 2.0),
            border_color(theme.tile.border),
            text_color(theme.text.muted)
        )
        .child(render_blocks(
            id,
            children,
            base_text_style,
            theme,
            structure,
            member_name,
            counter,
        ))
        .into_any(),
        CachedBlock::Rule => {
            tailwind_div!(w_full, h(structure.divider_width), bg(theme.tile.border)).into_any()
        }
        CachedBlock::Image { .. } => tailwind_div!(text_color(theme.colors.warning))
            .child("Inline images are not supported yet")
            .into_any(),
    }
}

fn render_rich_text(
    id: &SharedString,
    text: &CachedRichText,
    base_text_style: &TextStyle,
    theme: &AppTheme,
    member_name: &dyn Fn(&UserId) -> (SharedString, Hsla),
    counter: &mut usize,
) -> AnyElement {
    if text.runs.is_empty() {
        return tailwind_div!(text_color(theme.text.normal))
            .child(text.text.clone())
            .into_any();
    }

    let paragraph_key = next_id(id, counter);

    let mut rendered = String::new();
    let mut runs = Vec::with_capacity(text.runs.len());
    let mut click_ranges: Vec<Range<usize>> = Vec::new();
    let mut click_actions: Vec<RunAction> = Vec::new();

    let mut offset = 0;
    for run in text.runs.iter() {
        let slice = text.text.get(offset..offset + run.len).unwrap_or_default();
        offset += run.len;

        let mut highlight = HighlightStyle {
            color: Some(theme.text.normal),
            font_weight: run.style.bold.then_some(FontWeight::BOLD),
            font_style: run.style.italic.then_some(FontStyle::Italic),
            strikethrough: run.style.strikethrough.then(|| StrikethroughStyle {
                thickness: px(1.0),
                color: None,
            }),
            underline: run.style.underline.then(|| UnderlineStyle {
                thickness: px(1.0),
                color: None,
                wavy: false,
            }),
            ..Default::default()
        };

        if run.style.code {
            highlight.color = Some(theme.accent);
        }

        let run_text: SharedString =
            if let Some(CachedLink::Pill(CachedPill::User(user_id))) = &run.style.link {
                let (name, pill_color) = member_name(user_id);
                highlight.color = Some(pill_color);
                highlight.font_weight = Some(FontWeight::BOLD);
                highlight.background_color = Some(pill_color.alpha(0.16));
                format!("@{}", name.replace(' ', "\u{a0}")).into()
            } else if run.style.spoiler.is_some() {
                // TODO: Reveal on click
                highlight.background_color = Some(theme.text.dim);
                highlight.color = Some(theme.text.dim);
                slice.into()
            } else {
                if run.style.link.is_some() {
                    highlight.color = Some(theme.accent);
                }
                slice.into()
            };

        let start = rendered.len();
        rendered.push_str(&run_text);
        let end = rendered.len();

        if let Some(action) = link_action(&run.style.link) {
            click_ranges.push(start..end);
            click_actions.push(action);
        }

        runs.push(
            base_text_style
                .clone()
                .highlight(highlight)
                .to_run(run_text.len()),
        );
    }

    InteractiveText::new(
        ElementId::Name(paragraph_key),
        StyledText::new(rendered).with_runs(runs),
    )
    .on_click(click_ranges, move |ix, _window, cx| {
        match &click_actions[ix] {
            RunAction::OpenUrl(href) => cx.open_url(href),
        }
    })
    .into_any_element()
}

/// What activating a run (a click on a link/pill) does.
enum RunAction {
    OpenUrl(SharedString),
}

fn link_action(link: &Option<CachedLink>) -> Option<RunAction> {
    match link {
        Some(CachedLink::Url(href)) => Some(RunAction::OpenUrl(href.clone())),
        Some(CachedLink::Pill(pill)) => Some(RunAction::OpenUrl(pill_href(pill))),
        None => None,
    }
}

fn pill_href(pill: &CachedPill) -> SharedString {
    match pill {
        CachedPill::User(id) => format!("https://matrix.to/#/{id}").into(),
        CachedPill::Room(id) => format!("https://matrix.to/#/{id}").into(),
        CachedPill::Event { room, event } => format!("https://matrix.to/#/{room}/{event}").into(),
    }
}
