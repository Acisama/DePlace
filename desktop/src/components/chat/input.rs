use std::{collections::HashMap, path::PathBuf, sync::Arc};

use deplace_core::{
    helpers::RoomPlaceholderExt,
    matrix_api::messages::MatrixAttachment,
    settings::DataSizeUnit,
    state::{AppState, MembershipMap},
};
use gpui::{
    AppContext, ClickEvent, Context, Entity, FocusHandle, Focusable, ImageFormat,
    InteractiveElement, IntoElement, MouseButton, MouseClickEvent, MouseDownEvent, ParentElement,
    PathPromptOptions, Render, SharedString, StatefulInteractiveElement, Styled, TextOverflow,
    Window, prelude::FluentBuilder, px,
};
use gpui_component::{
    StyledExt,
    input::{Input, InputState},
    scroll::ScrollableElement,
};
use macros::tailwind_div;
use matrix_sdk::{Room, ruma::OwnedEventId};
use mime_guess::from_path;
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
    sync::watch::Receiver,
};
use uuid::Uuid;

use crate::{
    attachments::{Attachment, AttachmentPreview, AttachmentState},
    cache::{AvatarCache, ThumbnailCache},
    components::{
        ByteSize, CustomStyles,
        chat::{message::CachedTimelineItem, timeline::SendMessage},
        profiles::render_icon,
    },
    helpers::file_color,
    things::DeplaceThings,
    watch_bridge::notify_on_change,
};

pub struct ChatInputView {
    pub chat_input: Entity<InputState>,
    _membership_map: Receiver<MembershipMap>,

    attachments: HashMap<Uuid, Attachment>,

    data_size_unit: Receiver<DataSizeUnit>,

    hovered_button: Option<&'static str>,

    replying_to: Option<CachedTimelineItem>,
    state: AppState,
    active_room: Room,

    avatar_cache: AvatarCache,
    image_cache: ThumbnailCache,
}

impl ChatInputView {
    pub fn new(
        state: &AppState,
        window: &mut Window,
        cx: &mut Context<Self>,
        active_room: Room,
        // TODO: Move this into state
        avatar_cache: AvatarCache,
        image_cache: ThumbnailCache,
    ) -> Self {
        let membership_map = state.membership_map();
        let data_size_unit = state.settings().watch_data_size_unit();

        let chat_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(1, 10)
                .placeholder(format!(
                    "Message {} ...",
                    active_room.get_input_placeholder()
                ))
        });

        notify_on_change(data_size_unit.clone(), cx);
        notify_on_change(membership_map.clone(), cx);

        Self {
            chat_input: chat_input.clone(),
            _membership_map: membership_map.clone(),
            data_size_unit: data_size_unit.clone(),

            attachments: HashMap::new(),

            hovered_button: None,
            replying_to: None,
            state: state.clone(),
            active_room,
            avatar_cache,
            image_cache,
        }
    }

    pub fn reply_to(&mut self, replying_to: CachedTimelineItem) {
        self.replying_to = Some(replying_to);
    }

    /// Stop replying
    ///
    /// This returns `true` if a reply was removed, otherwise not
    pub fn remove_reply(&mut self) -> bool {
        match self.replying_to {
            None => false,
            Some(_) => {
                self.replying_to = None;
                true
            }
        }
    }
}

impl Focusable for ChatInputView {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.chat_input.focus_handle(cx)
    }
}

#[derive(Clone)]
pub enum SendEvent {
    SendMessage {
        text: String,
        attachments: Vec<MatrixAttachment>,
        in_reply_to: Option<OwnedEventId>,
    },
    EditMessage {
        target_id: OwnedEventId,
        text: String,
    },
}

impl gpui::EventEmitter<SendEvent> for ChatInputView {}

impl Render for ChatInputView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.app_theme().clone();
        let structure = cx.structure().clone();

        let input_focus_handle = self.chat_input.read(cx).focus_handle(cx);
        let input_focused = input_focus_handle.is_focused(window);

        let (input_bg, input_border) = if input_focused {
            (theme.input.focus_background, theme.input.focused_border)
        } else {
            (theme.input.background, theme.tile.border)
        };

        let icon_size = structure.chat.input_height - structure.small_gap * 2.0;

        let attachments = (!self.attachments.is_empty()).then_some(self.attachments.clone());
        let has_reply = self.replying_to.is_some();
        let has_attachments = attachments.is_some();

        // Pre-render to drop borrow of cx
        let rendered_reply = self.replying_to.as_ref().map(|replying_to| {
            let room_id = self.active_room.room_id();
            let membership_map = self._membership_map.borrow();
            let avatar_cache = &self.avatar_cache;
            let image_cache = &self.image_cache;

            replying_to.render(
                window,
                cx,
                |_: Arc<OwnedEventId>, _: SharedString| {},
                room_id,
                &membership_map,
                avatar_cache,
                image_cache,
                false,
                None,
                false,
                &self.state,
            )
        });

        let view = tailwind_div!(w_full, flex, flex_col)
            .when_some(rendered_reply, |el, rendered_reply| {
                el.child(
                    tailwind_div!(
                        flex,
                        flex_row,
                        items_center,
                        justify_between,
                        gap(structure.small_gap),
                        paddings(structure.small_gap),
                        bg(input_bg),
                        border_1,
                        border_b_0,
                        border_color(theme.tile.border),
                        rounded_t(structure.inner_border_radius),
                        w_full,
                        overflow_hidden
                    )
                    .child(
                        tailwind_div!(
                            flex,
                            flex_row,
                            items_center,
                            gap(structure.small_gap),
                            overflow_hidden,
                            flex_1
                        )
                        .child(
                            tailwind_div!(
                                text_color(theme.text.dim),
                                text_size(structure.chat.small_text_size),
                                flex_shrink_0
                            )
                            .child("Replying to:"),
                        )
                        .child(
                            tailwind_div!(max_h(px(80.0)), overflow_y_hidden, flex_1)
                                .child(rendered_reply),
                        ),
                    )
                    .child(
                        tailwind_div!(
                            w(icon_size),
                            h(icon_size),
                            flex,
                            items_center,
                            justify_center,
                            rounded(structure.semi_border_radius()),
                            cursor_pointer,
                            text_color(theme.text.dim),
                            hover(text_color(theme.text.normal))
                        )
                        .id("cancel_reply_button")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.remove_reply();
                            cx.notify();
                        }))
                        .child(render_icon(
                            phosphor_svgs::icon::x::REGULAR,
                            icon_size - structure.small_gap * 2.0,
                        )),
                    ),
                )
            })
            .when_some(attachments, |el, attachments| {
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
                        overflow_x_scrollbar
                    )
                    .when(!has_reply, |el| el.rounded_t(structure.inner_border_radius))
                    .children(attachments.iter().map(|(id, a)| {
                        tailwind_div!(
                            rounded(structure.semi_border_radius()),
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
                            .child(
                                a.render_preview(&theme.clone(), structure.semi_border_radius()),
                            ),
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
                            .child(
                                tailwind_div!(text_color(theme.text.muted))
                                    .child(a.size.get(&self.data_size_unit.borrow())),
                            ),
                        )
                        .id(id.to_string())
                        .on_aux_click(cx.listener({
                            let id = *id;
                            move |view, ev, _, cx| {
                                if matches!(
                                    ev,
                                    ClickEvent::Mouse(MouseClickEvent {
                                        down: MouseDownEvent {
                                            button: MouseButton::Middle,
                                            ..
                                        },
                                        ..
                                    })
                                ) && view.attachments.remove(&id).is_some()
                                {
                                    cx.notify();
                                }
                            }
                        }))
                    })),
                )
            });

        let chat_input_button = |svg: &'static str, id: &'static str| {
            let hovered = self.hovered_button == Some(id);

            tailwind_div!(
                w(icon_size),
                h(icon_size),
                flex,
                items_center,
                justify_center,
                rounded(structure.semi_border_radius()),
                cursor_pointer,
                text_color(if hovered {
                    theme.text.normal
                } else {
                    theme.text.dim
                })
            )
            .id(id)
            .on_click(cx.listener(move |_, _, _, cx| {
                let paths_rx = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: true,
                    prompt: Some(SharedString::new("Upload files")),
                });

                let theme = cx.app_theme().clone();
                cx.spawn(async move |this, cx| {
                    let Ok(Ok(Some(paths))) = paths_rx.await else {
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

                        let mut update_attachment = |attachment: &Attachment| {
                            if let Err(e) = this.update(cx, |this, cx| {
                                this.attachments.insert(id, attachment.clone());
                                cx.notify();
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
        };

        view.child(
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
            .when(!has_reply && !has_attachments, |el| {
                el.rounded_t(structure.inner_border_radius)
            })
            .track_focus(&input_focus_handle)
            .on_action(cx.listener(|this, _event: &SendMessage, _window, cx| {
                let text = this.chat_input.read(cx).text().to_string();
                let attachments: Vec<MatrixAttachment> = this
                    .attachments
                    .values()
                    .cloned()
                    .filter_map(|a| {
                        if let AttachmentState::Loaded(bytes) = a.state {
                            Some(MatrixAttachment {
                                filename: a.name.into(),
                                mime_type: (*a.mime_type).clone(),
                                data: (*bytes).clone(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();

                if text.trim().is_empty() && attachments.is_empty() {
                    return;
                }

                let in_reply_to = this
                    .replying_to
                    .as_ref()
                    .map(|r| r.owned_event_id())
                    .flatten();

                cx.emit(SendEvent::SendMessage {
                    text,
                    attachments,
                    in_reply_to,
                });

                this.chat_input.update(cx, |input, cx| {
                    input.set_value("", _window, cx);
                });
                this.attachments.clear();
                this.remove_reply();
                cx.notify();
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
