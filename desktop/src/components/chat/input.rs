use std::{collections::HashMap, path::PathBuf, sync::Arc};

use deplace_core::{
    helpers::RoomPlaceholderExt,
    state::{AppState, MembershipMap},
};
use gpui::{
    Animation, AnimationExt, AppContext, Context, ElementId, Entity, FocusHandle, Focusable, Hsla,
    ImageFormat, InteractiveElement, IntoElement, ParentElement, PathPromptOptions, Render,
    SharedString, StatefulInteractiveElement, Styled, TextOverflow, Window, ease_in_out,
    prelude::FluentBuilder, transparent_black,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputEvent, InputState, RopeExt as _},
    scroll::ScrollableElement,
};
use macros::tailwind_div;
use matrix_sdk::{Room, ruma::OwnedRoomId};
use mime_guess::from_path;
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
    sync::watch::Receiver,
};
use uuid::Uuid;

use crate::{
    components::{ByteSize, CustomStyles, chat::SendMessage, profiles::render_icon},
    helpers::file_color,
    room_state::{Attachment, AttachmentPreview, AttachmentState, RoomStateStore},
    theme::DeplaceThings,
    watch_bridge::{execute_on_change, notify_on_change},
};

pub struct ChatInputBar {
    pub chat_input: Entity<InputState>,
    active_room: Receiver<Option<Room>>,
    membership_map: Receiver<MembershipMap>,

    room_store: RoomStateStore,

    hovered_button: Option<&'static str>,
}

fn lerp_hsla(from: Hsla, to: Hsla, t: f32) -> Hsla {
    Hsla {
        h: from.h + (to.h - from.h) * t,
        s: from.s + (to.s - from.s) * t,
        l: from.l + (to.l - from.l) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

impl ChatInputBar {
    pub fn new(
        state: &AppState,
        window: &mut Window,
        cx: &mut Context<Self>,
        room_store: RoomStateStore,
    ) -> Self {
        let active_room = state.active_room();
        let membership_map = state.membership_map();

        let chat_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(1, 10)
                .placeholder(
                    active_room
                        .borrow()
                        .clone()
                        .get_input_placeholder(&membership_map.borrow()),
                )
        });

        notify_on_change(room_store.subscribe(), cx);

        let view = Self {
            chat_input: chat_input.clone(),
            active_room: active_room.clone(),
            membership_map: membership_map.clone(),

            room_store,

            hovered_button: None,
        };

        cx.subscribe(&chat_input, |view: &mut Self, chat_input, event, cx| {
            if let InputEvent::Change = event
                && let Some(room) = view.active_room.borrow().clone()
            {
                view.room_store.mutate(room.room_id(), |room_state| {
                    room_state.chat_input = chat_input.read(cx).text().to_string().into();
                });
            }
        })
        .detach();

        execute_on_change(
            active_room.clone(),
            cx,
            "chat_input",
            move |view, window, cx, room, _| {
                view.on_room_change(window, cx, room.map(|r| r.room_id().to_owned()))
            },
        );

        view
    }

    fn on_room_change(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        room_id: Option<OwnedRoomId>,
    ) {
        let value = if let Some(room_id) = room_id {
            self.room_store.get(&room_id).unwrap_or_default().chat_input
        } else {
            SharedString::default()
        };

        self.chat_input.update(cx, |input, cx| {
            input.set_value(value, window, cx);
            let end = input.text().offset_to_position(input.text().len());
            input.set_cursor_position(end, window, cx);
        });

        self.chat_input.focus_handle(cx).focus(window, cx);

        self.update_placeholder(window, cx);
    }

    fn update_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let active_room = self.active_room.borrow().clone();
        let map = self.membership_map.borrow().clone();

        self.chat_input.update(cx, |input, cx| {
            input.set_placeholder(active_room.get_input_placeholder(&map), window, cx)
        });
    }
}

impl Focusable for ChatInputBar {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.chat_input.focus_handle(cx)
    }
}

pub struct SendMessageEvent {
    pub text: String,
}

impl gpui::EventEmitter<SendMessageEvent> for ChatInputBar {}

impl Render for ChatInputBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme();
        let structure = cx.structure();

        let input_focus_handle = self.chat_input.read(cx).focus_handle(cx);
        let input_focused = input_focus_handle.is_focused(window);

        let (input_bg, input_border) = if input_focused {
            (theme.input.focus_background, theme.input.focused_border)
        } else {
            (theme.input.background, theme.tile.border)
        };

        let icon_size = structure.chat.input_height - structure.small_gap * 2.0;
        let smaller_border_radius = (structure.inner_border_radius + structure.small_gap) / 2.0;

        let chat_input_button = |svg: &'static str, id: &'static str| {
            let hovered = self.hovered_button == Some(id);
            let (from, to) = if hovered {
                (transparent_black(), theme.solid_hover_bg)
            } else {
                (theme.solid_hover_bg, transparent_black())
            };

            tailwind_div!(
                w(icon_size),
                h(icon_size),
                flex,
                items_center,
                justify_center,
                rounded((structure.inner_border_radius + structure.smaller_border_radius) / 2.0),
                cursor_pointer,
                text_color(if hovered {
                    theme.text.normal
                } else {
                    theme.text.dim
                })
            )
            .id(id)
            .on_click(cx.listener(move |view, _ev, _window, cx| {
                let paths_rx = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: true,
                    prompt: Some(SharedString::new("Upload files")),
                });

                let room_id = view
                    .active_room
                    .borrow()
                    .as_ref()
                    .map(|r| r.room_id().to_owned());

                let theme = cx.app_theme().clone();
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(Some(paths))) = paths_rx.await else {
                        return;
                    };
                    let Some(room_id) = room_id else {
                        return;
                    };

                    let paths_map: HashMap<Uuid, PathBuf> =
                        paths.into_iter().map(|p| (Uuid::now_v7(), p)).collect();

                    for (id, path) in paths_map {
                        let name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let mime_type = from_path(&path).first_or_octet_stream();

                        let mut attachment = Attachment::new(&name, mime_type.clone());

                        let room_id = room_id.clone();
                        let mut update_attachment = |attachment: &Attachment| {
                            if let Err(e) = this.update(cx, |this, _cx| {
                                this.room_store.mutate(&room_id, |room_state| {
                                    room_state.attachments.insert(id, attachment.clone());
                                });
                            }) {
                                tracing::error!("Failed to update attachment state: {}", e);
                            }
                        };

                        update_attachment(&attachment);

                        let mut file = match File::open(&path).await {
                            Ok(f) => f,
                            Err(e) => {
                                attachment.state = AttachmentState::Failed(e.to_string().into());
                                update_attachment(&attachment);
                                break;
                            }
                        };

                        let mut buffer = [0; 400];

                        let bytes_read = file.read(&mut buffer).await;

                        let color = bytes_read
                            .map(|s| infer::get(&buffer[..s]))
                            .map_err(|e| tracing::error!("Failed to read first 400 bytes: {e}"))
                            .ok()
                            .flatten()
                            .map(file_color)
                            .unwrap_or(theme.colors.unknown);

                        let metadata = match file.metadata().await {
                            Ok(m) => m,
                            Err(e) => {
                                attachment.state = AttachmentState::Failed(e.to_string().into());
                                update_attachment(&attachment);
                                break;
                            }
                        };

                        attachment.size = ByteSize::new(metadata.len());
                        update_attachment(&attachment);

                        match fs::read(&path).await {
                            Ok(bytes) => {
                                tracing::trace!("Loaded attachment: {}", path.display());
                                attachment.preview = if let Some(format) =
                                    ImageFormat::from_mime_type(mime_type.as_ref())
                                {
                                    AttachmentPreview::Image(Arc::new(gpui::Image::from_bytes(
                                        format,
                                        bytes.clone(),
                                    )))
                                } else if let Some(extension) =
                                    path.extension().map(|e| e.to_string_lossy().to_string())
                                {
                                    AttachmentPreview::Extention {
                                        extension: extension.to_uppercase().into(),
                                        color,
                                    }
                                } else {
                                    AttachmentPreview::Unknown
                                };
                                attachment.state = AttachmentState::Loaded(Arc::new(bytes));
                                update_attachment(&attachment);
                            }
                            Err(e) => {
                                attachment.state = AttachmentState::Failed(e.to_string().into());
                                update_attachment(&attachment);
                            }
                        };
                    }
                })
                .detach();
            }))
            .on_hover(cx.listener(move |view, is_hovered: &bool, _, cx| {
                view.hovered_button = is_hovered.then_some(id);
                cx.notify();
            }))
            .child(render_icon(svg, icon_size - structure.small_gap * 2.0))
            .with_animation(
                ElementId::Name(format!("{id}-hover-{hovered}").into()),
                Animation::new(theme.hover_animation_duration).with_easing(ease_in_out),
                move |el, delta| el.bg(lerp_hsla(from, to, delta)),
            )
        };

        let active_room = self.active_room.borrow().clone();
        let active_room_id = active_room.as_ref().map(|r| r.room_id().to_owned());

        let attachments =
            active_room_id.and_then(|id| self.room_store.get(&id).map(|state| state.attachments));

        tailwind_div!(w_full, flex, flex_col)
            .when_some(attachments.clone(), |el, attachments| {
                let (at_w, at_h) = structure.chat.attachment_preview_dimensions;

                el.child(
                    tailwind_div!(
                        flex,
                        flex_row,
                        gap(structure.small_gap),
                        paddings(structure.small_gap),
                        bg(input_bg),
                        border_1,
                        border_b_0,
                        border_color(theme.tile.border),
                        rounded_t(structure.inner_border_radius),
                        overflow_x_scrollbar
                    )
                    .children(attachments.iter().map(|(id, a)| {
                        tailwind_div!(
                            rounded(smaller_border_radius),
                            border_1,
                            border_color(theme.tile.border),
                            cursor_pointer,
                            w(at_w),
                            overflow_hidden,
                            bg(theme.solid_bg),
                            hover(bg(theme.solid_hover_bg))
                        )
                        .child(
                            tailwind_div!(
                                w_full,
                                h(at_h),
                                border_b_1,
                                border_color(theme.tile.border),
                                overflow_hidden,
                                text_size(structure.chat.text_size * 1.5),
                                font_extrabold,
                            )
                            .child(a.render_preview(theme, smaller_border_radius)),
                        )
                        .child(
                            tailwind_div!(
                                paddings(structure.small_gap),
                                flex,
                                flex_col,
                                w_full,
                                truncate,
                                text_overflow(TextOverflow::Truncate("...".into())),
                                text_size(structure.chat.small_text_size)
                            )
                            .child(tailwind_div!(text_color(theme.text.dim)).child(a.name.clone()))
                            // TODO: Respect setting
                            .child(
                                tailwind_div!(text_color(theme.text.muted))
                                    .child(a.size.bytes_str.clone()),
                            ),
                        )
                        .id(id.to_string())
                    })),
                )
            })
            .child(
                tailwind_div!(
                    min_h(structure.chat.input_height),
                    flex,
                    flex_row,
                    items_center,
                    w_full,
                    rounded_b(structure.inner_border_radius),
                    paddings(structure.small_gap),
                    text_size(structure.chat.text_size),
                    gap(structure.small_gap),
                    border_1,
                    border_color(input_border),
                    bg(input_bg)
                )
                .when(attachments.is_none_or(|a| a.is_empty()), |el| {
                    el.rounded_t(structure.inner_border_radius)
                })
                .track_focus(&input_focus_handle)
                .on_action(cx.listener(|this, _event: &SendMessage, _window, cx| {
                    let text = this.chat_input.read(cx).text().to_string();
                    if !text.trim().is_empty() {
                        cx.emit(SendMessageEvent { text });
                    }
                    this.chat_input.update(cx, |input, cx| {
                        input.set_value("", _window, cx);
                    })
                }))
                .child(chat_input_button(
                    phosphor_svgs::icon::plus::REGULAR,
                    "chat_file_icon",
                ))
                .child(
                    Input::new(&self.chat_input)
                        .p_0()
                        .bg_transparent()
                        .border_transparent()
                        .text_color(theme.text.normal),
                ),
            )
    }
}
