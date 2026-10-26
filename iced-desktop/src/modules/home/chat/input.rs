use deplace_core::{
    helpers::RoomPlaceholderExt,
    keybinds::Keybinds,
    matrix_api::{attachments::MatrixAttachment, messages::RoomSendingExt},
};
use iced::{
    Length,
    border::{Radius, radius},
    widget::{Id, operation, text_editor},
};
use image::ImageReader;
use macros::iced_cache;
use matrix_sdk_ui::Timeline;
use std::io::Cursor;
use uuid::Uuid;

use crate::{
    common::*,
    components::{InsetShadow, help_mode::ChatHelpKey, phosphor_icon},
};

use super::timeline::messages::MessageEvent;

#[derive(Debug, Clone)]
pub enum ThumbnailType {
    Image(ImageHandle),
    Extention(String),
}

#[derive(Debug, Clone)]
pub struct Attachment {
    id: Uuid,
    thumbnail: ThumbnailType,
    attachment: MatrixAttachment,
    // True while the real attachment bytes (e.g. a clipboard image still being
    // PNG-encoded in the background) aren't ready yet. The thumbnail is already
    // shown, but a pending attachment must not be sent until this clears.
    pending: bool,
}

impl Hash for Attachment {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.pending.hash(state);
        self.attachment.filename.hash(state);
        self.attachment.mime_type.hash(state);
    }
}

impl From<MatrixAttachment> for Attachment {
    fn from(attachment: MatrixAttachment) -> Self {
        let thumbnail = match generate_thumbnail(&attachment.data, &attachment.mime_type) {
            Some(img) => ThumbnailType::Image(img),
            None => ThumbnailType::Extention(
                attachment
                    .filename
                    .split('.')
                    .next_back()
                    .unwrap_or("txt")
                    .to_string()
                    .to_uppercase(),
            ),
        };
        Self {
            id: Uuid::now_v7(),
            thumbnail,
            attachment,
            pending: false,
        }
    }
}

// Uploading a full-resolution decode straight to the GPU just to draw a small
// preview square is what stalled the window when picking large images: the
// texture upload runs synchronously on the render thread during `view()`.
const ATTACHMENT_THUMBNAIL_SIZE: u32 = 256;

fn generate_thumbnail(data: &[u8], mime_type: &mime::Mime) -> Option<ImageHandle> {
    if mime_type.type_() != mime::IMAGE {
        return None;
    }

    let img = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .thumbnail(ATTACHMENT_THUMBNAIL_SIZE, ATTACHMENT_THUMBNAIL_SIZE)
        .to_rgba8();

    Some(ImageHandle::from_rgba(
        img.width(),
        img.height(),
        img.into_raw(),
    ))
}

// Same idea as `generate_thumbnail`, but for raw pixels (e.g. straight off the
// clipboard) instead of encoded bytes that need decoding first.
fn downscale_thumbnail(width: u32, height: u32, rgba: &[u8]) -> Option<ImageHandle> {
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let thumb = image::DynamicImage::ImageRgba8(img)
        .thumbnail(ATTACHMENT_THUMBNAIL_SIZE, ATTACHMENT_THUMBNAIL_SIZE)
        .to_rgba8();

    Some(ImageHandle::from_rgba(
        thumb.width(),
        thumb.height(),
        thumb.into_raw(),
    ))
}

// Each attachment gets its own blocking task so decoding thumbnails for
// several files happens in parallel, and each attachment shows up in the
// input bar as soon as it is ready instead of all at once.
fn attachments_task(attachments: Vec<MatrixAttachment>) -> Task<InputMessage> {
    Task::batch(attachments.into_iter().map(|attachment| {
        Task::future(async move {
            match tokio::task::spawn_blocking(move || attachment.into()).await {
                Ok(attachment) => InputMessage::AttachmentAdded(vec![Arc::new(attachment)]),
                Err(e) => {
                    tracing::error!("Failed to build attachment: {}", e);
                    InputMessage::None
                }
            }
        })
    }))
}

fn attachment_task(attachment: Option<MatrixAttachment>) -> Task<InputMessage> {
    Task::future(async move {
        match attachment {
            Some(attachment) => {
                match tokio::task::spawn_blocking(move || attachment.into()).await {
                    Ok(attachment) => InputMessage::AttachmentAdded(vec![Arc::new(attachment)]),
                    Err(e) => {
                        tracing::error!("Failed to build attachment: {}", e);
                        InputMessage::None
                    }
                }
            }
            None => InputMessage::None,
        }
    })
}

#[derive(Debug, Clone)]
pub enum InputMessage {
    None,
    TextAction(text_editor::Action),
    SendMessage,
    Cancel,
    BackspacePressed { ctrl: bool },
    RemoveReplying,
    HelpOver(Option<HelpKey>),
    OpenAttachmentPicker,
    OpenCompressedAttachmentPicker,
    AttachmentAdded(Vec<Arc<Attachment>>),
    Paste,
    AttachmentReady(Uuid, MatrixAttachment),
    AttachmentFailed(Uuid),
    RemoveAttachment(usize),
}

pub enum InputAction {
    SendMessage(Task<()>),
    RemoveReplying,
    HelpOver(Option<HelpKey>),
    Perform(Task<InputMessage>),
}

#[iced_cache(Clone)]
pub struct ChatInput {
    state: AppState,
    pub timeline: Option<Arc<Timeline>>,

    #[hash]
    room_id: OwnedRoomId,

    room_watchers: RoomWatchers,

    id: Id,

    /// The `[crate::components::IcedWidget]` crate requires 'static lifetimes for caching, but `text_editor` requires a reference to the content since it stores the data internally.
    content: *mut text_editor::Content,
    #[hash]
    replying_to: Option<(Arc<MessageEvent>, OwnedEventId)>,

    #[hash]
    attachments: Vec<Arc<Attachment>>,

    keybinds: Receiver<Keybinds>,
}

impl ChatInput {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        let room_id = room.room_id().to_owned();
        Self {
            state: state.clone(),
            timeline: None,

            room_watchers: state.room_watchers(hashing::hash_room_default(room_id.clone())),
            room_id,

            id: Id::unique(),
            content: Box::leak(Box::new(text_editor::Content::new())),
            replying_to: None,

            attachments: Vec::new(),

            keybinds: state.keybinds(),
        }
    }

    pub fn set_replies_to(&mut self, message: Arc<MessageEvent>, event_id: OwnedEventId) {
        self.replying_to = Some((message, event_id));
    }

    fn content(&self) -> &'static text_editor::Content {
        // The pointer is never changed/freed, so this is safe
        unsafe { &*self.content }
    }

    pub fn get_raw_content_pointer(&self) -> *mut text_editor::Content {
        self.content
    }

    pub fn focus<T>(&self) -> Task<T> {
        operation::focus::<T>(self.id.clone())
    }
}

impl IcedWidget<InputMessage, InputAction> for ChatInput {
    fn update(&mut self, message: InputMessage) -> Option<InputAction> {
        match message {
            InputMessage::RemoveAttachment(index) => {
                self.attachments.remove(index);
                Some(InputAction::Perform(self.focus()))
            }
            InputMessage::AttachmentAdded(attachments) => {
                self.attachments.extend(attachments);
                Some(InputAction::Perform(self.focus()))
            }
            InputMessage::OpenAttachmentPicker => Some(InputAction::Perform(
                self.focus()
                    .chain(Task::future(MatrixAttachment::from_files_picker()))
                    .then(attachments_task),
            )),
            InputMessage::OpenCompressedAttachmentPicker => {
                let state = self.state.clone();
                Some(InputAction::Perform(
                    self.focus()
                        .chain(Task::future(
                            MatrixAttachment::compressed_from_folder_picker(state),
                        ))
                        .then(attachment_task),
                ))
            }
            InputMessage::Paste => {
                let task = iced::clipboard::read_image().then(|result| match result {
                    Ok(image) => {
                        let id = Uuid::now_v7();

                        // Downscaling still goes through spawn_blocking: uploading the
                        // clipboard image's full resolution straight to the GPU for a
                        // small preview box is what caused the freeze with large images.
                        let show_now = Task::future({
                            let image = image.clone();
                            async move {
                                let thumbnail = tokio::task::spawn_blocking(move || {
                                    downscale_thumbnail(
                                        image.size.width,
                                        image.size.height,
                                        &image.rgba,
                                    )
                                })
                                .await
                                .ok()
                                .flatten()
                                .map(ThumbnailType::Image)
                                .unwrap_or_else(|| ThumbnailType::Extention("IMG".to_string()));

                                InputMessage::AttachmentAdded(vec![Arc::new(Attachment {
                                    id,
                                    thumbnail,
                                    attachment: MatrixAttachment::from_bytes(
                                        "pasted-image.png",
                                        mime::IMAGE_PNG,
                                        Vec::new(),
                                    ),
                                    pending: true,
                                })])
                            }
                        });

                        let encode = Task::future(async move {
                            match tokio::task::spawn_blocking(move || {
                                MatrixAttachment::from_rgba(
                                    "pasted-image.png",
                                    image.size.width,
                                    image.size.height,
                                    &image.rgba,
                                )
                            })
                            .await
                            {
                                Ok(Ok(attachment)) => InputMessage::AttachmentReady(id, attachment),
                                Ok(Err(e)) => {
                                    tracing::error!("Failed to encode clipboard image: {}", e);
                                    InputMessage::AttachmentFailed(id)
                                }
                                Err(e) => {
                                    tracing::error!("Failed to encode clipboard image: {}", e);
                                    InputMessage::AttachmentFailed(id)
                                }
                            }
                        });

                        Task::batch([show_now, encode])
                    }
                    Err(_) => iced::clipboard::read_text().then(|result| match result {
                        Ok(text) => Task::done(InputMessage::TextAction(
                            text_editor::Action::Edit(text_editor::Edit::Paste(text)),
                        )),
                        Err(_) => Task::done(InputMessage::None),
                    }),
                });

                Some(InputAction::Perform(task))
            }
            InputMessage::AttachmentReady(id, attachment) => {
                if let Some(existing) = self.attachments.iter_mut().find(|a| a.id == id) {
                    *existing = Arc::new(Attachment {
                        id,
                        thumbnail: existing.thumbnail.clone(),
                        attachment,
                        pending: false,
                    });
                }
                None
            }
            InputMessage::AttachmentFailed(id) => {
                self.attachments.retain(|a| a.id != id);
                None
            }
            InputMessage::HelpOver(help) => Some(InputAction::HelpOver(help)),
            InputMessage::None => None,
            InputMessage::TextAction(action) => {
                // The rendered view is dropped before this is called, so there is only one (mutable) reference to `content` at a time
                unsafe { &mut *self.content }.perform(action);
                None
            }
            InputMessage::RemoveReplying => {
                self.replying_to = None;
                Some(InputAction::RemoveReplying)
            }
            InputMessage::Cancel => {
                self.replying_to = None;
                None
            }
            InputMessage::SendMessage => {
                let Some(timeline) = self.timeline.clone() else {
                    tracing::warn!(
                        "Tried to send message in room {} without a timeline",
                        self.room_id
                    );
                    return None;
                };

                let text = self.content().text();
                let text = (!text.is_empty()).then_some(text);

                let replying_to = if let Some((_, id)) = &self.replying_to {
                    Some(id.clone())
                } else {
                    None
                };

                let content = unsafe { &mut *self.content };
                content.perform(text_editor::Action::SelectAll);
                content.perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                self.replying_to = None;

                // Attachments still being encoded (e.g. a clipboard image) aren't sendable
                // yet; leave them in place so they go out with the next send instead of
                // being sent with no data. `MatrixAttachment::data` is `Bytes`, so cloning
                // it here is a cheap refcount bump, not a copy of the file.
                let still_pending: Vec<_> = self
                    .attachments
                    .iter()
                    .filter(|a| a.pending)
                    .cloned()
                    .collect();

                let attachments: Vec<MatrixAttachment> = self
                    .attachments
                    .iter()
                    .filter(|a| !a.pending)
                    .map(|attachment| attachment.attachment.clone())
                    .collect();
                self.attachments = still_pending;

                Some(InputAction::SendMessage(Task::future(async move {
                    let mut next_replying_to = None;
                    if let Some(text) = text {
                        if let Err(e) = timeline.send_message(text, replying_to).await {
                            tracing::warn!("Failed to send message: {}", e);
                        }
                        next_replying_to = None;
                    }

                    for attachment in attachments {
                        if let Err(e) = timeline
                            .send_deplace_attachment(attachment, next_replying_to)
                            .await
                        {
                            tracing::warn!("Failed to send message: {}", e);
                        }
                        next_replying_to = None;
                    }
                })))
            }
            InputMessage::BackspacePressed { ctrl } => {
                let iced::advanced::text::Position { line, index } =
                    self.content().cursor().position;
                if line == 0 && index == 0 {
                    if self.replying_to.is_some() {
                        return Some(InputAction::Perform(Task::done(
                            InputMessage::RemoveReplying,
                        )));
                    } else if !self.attachments.is_empty() {
                        if !ctrl {
                            return Some(InputAction::Perform(Task::done(
                                InputMessage::RemoveAttachment(self.attachments.len() - 1),
                            )));
                        } else {
                            let mut tasks = vec![];
                            for _ in 0..self.attachments.len() {
                                tasks.push(Task::done(InputMessage::RemoveAttachment(0)));
                            }
                            return Some(InputAction::Perform(Task::batch(tasks)));
                        }
                    }
                } else {
                    if ctrl {
                        unsafe {
                            self.content.as_mut()?.perform(text_editor::Action::Edit(
                                text_editor::Edit::BackspaceWord,
                            ));
                        }
                    } else {
                        unsafe {
                            self.content
                                .as_mut()?
                                .perform(text_editor::Action::Edit(text_editor::Edit::Backspace));
                        }
                    }
                }
                None
            }
        }
    }

    fn view(
        &self,
        theme: Theme,
        structure: Structure,
        help_state: HelpState<HelpKey>,
    ) -> iced::Element<'static, InputMessage> {
        let Some(room) = self.room_watchers.get_room(&self.room_id) else {
            return w::Space::new().into();
        };

        let keybinds = self.keybinds.clone();
        let line_height = structure.chat.text_size;

        let button_size = line_height + structure.small_gap * 2.0;

        let reply_bar_size = structure.chat.text_size + structure.small_gap * 2.0;

        let help_view = HelpView::new(help_state, theme, InputMessage::HelpOver);

        let input_button = |svg: &'static str, key, help, hover_tip, message| {
            w::container(
                help_view
                    .call(
                        key,
                        help,
                        themed_tooltip_text(
                            w::button(phosphor_icon(svg, line_height))
                                .style(move |_, status| ButtonStyle {
                                    text_color: if status.active() {
                                        theme.text.normal.into()
                                    } else {
                                        theme.text.dim.into()
                                    },
                                    background: status
                                        .active()
                                        .then_some(theme.solid_hover_bg.into()),
                                    border: border::rounded(structure.semi_border_radius()),
                                    ..Default::default()
                                })
                                .on_press(message)
                                .padding(structure.small_gap),
                            hover_tip,
                            structure,
                            theme,
                        ),
                    )
                    .radius(structure.semi_border_radius()),
            )
            .padding(structure.small_gap)
        };

        let max_lines = 10.0;

        let replies_to = if let Some((msg, _)) = &self.replying_to {
            Some(msg.clone())
        } else {
            None
        };
        let is_replying_to = self.replying_to.is_some();
        let has_attachments = !self.attachments.is_empty();

        let attachment_banner: Option<Element<'static, InputMessage>> =
            has_attachments.then(|| {
                let children = self
                    .attachments
                    .iter()
                    .enumerate()
                    .map(|(index, attachment)| {
                        let size = structure.chat.attachment_preview_size;

                        let image: Element<'static, InputMessage> =
                            match attachment.thumbnail.clone() {
                                ThumbnailType::Image(handle) => w::image(handle)
                                    .width(size)
                                    .height(size)
                                    .border_radius(structure.semi_border_radius())
                                    .into(),
                                ThumbnailType::Extention(ext) => {
                                    let color = DePlaceColor::from(ext.as_str()).to_iced();
                                    w::stack![
                                        w::container(pan(weighted_text(ext, Weight::Bold)
                                            .size((structure.large_font_size).max(1.0))))
                                        .width(size)
                                        .height(size)
                                        .center(size)
                                        .style(move |_| {
                                            w::container::Style {
                                                background: Some(color.scale_lightness(0.2).into()),
                                                text_color: Some(color),
                                                border: Border {
                                                    color,
                                                    width: 0.0,
                                                    radius: structure.semi_border_radius().into(),
                                                },
                                                ..Default::default()
                                            }
                                        }),
                                        Canvas::new(InsetShadow::new(
                                            structure.semi_border_radius(),
                                            color,
                                            size / 8.0,
                                            16
                                        ))
                                        .width(size)
                                        .height(size),
                                    ]
                                    .clip(true)
                                    .into()
                                }
                            };

                        help_view
                            .call(
                                HelpKey::Chat(ChatHelpKey::Attachment),
                                "An attachment for the current message",
                                w::column![
                                    w::mouse_area(image)
                                        .on_middle_press(InputMessage::RemoveAttachment(index)),
                                    pan(w::text(attachment.attachment.filename.clone())
                                        .size(structure.chat.text_size)
                                        .color(theme.text.normal))
                                    .width(size)
                                ],
                            )
                            .radius(radius(structure.semi_border_radius()).bottom(0.0))
                            .into()
                    });

                help_view
                    .call(
                        HelpKey::Chat(ChatHelpKey::Attachments),
                        "Attachments for the current message",
                        w::container(themed_scrollable(
                            w::Row::with_children(children)
                                .width(Fill)
                                .padding(structure.small_gap)
                                .spacing(structure.small_gap),
                            theme,
                            structure,
                        ))
                        .style(move |_| ContainerStyle {
                            background: Some(theme.solid_bg.into()),
                            text_color: Some(theme.text.normal.into()),
                            border: Border {
                                color: theme.border.into(),
                                width: structure.border_thickness,
                                radius: radius(structure.inner_border_radius).bottom(0.0),
                            },
                            ..Default::default()
                        }),
                    )
                    .radius(radius(structure.inner_border_radius).bottom(0.0))
                    .into()
            });

        let replying_banner: Option<Element<'static, InputMessage>> = replies_to.map(|msg| {
            let radius = radius(if !has_attachments {
                structure.inner_border_radius
            } else {
                0.0
            })
            .bottom(0.0);

            help_view
                .call(
                    HelpKey::Chat(ChatHelpKey::Input),
                    "The message being replied to",
                    w::container(w::column![
                        w::row![
                            w::container(
                                w::text("Replying to")
                                    .size(structure.chat.text_size)
                                    .color(theme.text.normal)
                            )
                            .width(Fill)
                            .padding(padding::left(structure.small_gap))
                            .center_y(reply_bar_size),
                            close_button(theme, structure, InputMessage::RemoveReplying)
                        ]
                        .width(Fill),
                        w::container(Space::new())
                            .style(move |_| ContainerStyle {
                                background: Some(theme.border.into()),
                                ..Default::default()
                            })
                            .height(structure.border_thickness)
                            .width(Fill),
                        w::container(
                            msg.view(
                                self.room_id.clone(),
                                theme,
                                structure,
                                true,
                                help_state,
                                0,
                                false
                            )
                            .map(|_| InputMessage::None)
                        )
                    ])
                    .style(move |_| ContainerStyle {
                        background: Some(theme.solid_bg.into()),
                        border: Border {
                            color: theme.border.into(),
                            width: structure.border_thickness,
                            radius,
                        },
                        ..Default::default()
                    }),
                )
                .radius(radius)
                .into()
        });

        help_view
            .call(
                HelpKey::Chat(ChatHelpKey::Input),
                "The chat input, you can write messages here",
                w::column![
                    attachment_banner,
                    replying_banner,
                    w::stack![
                        w::text_editor(self.content())
                            .id(self.id.clone())
                            .placeholder(format!("Message {}", room.get_input_placeholder()))
                            .padding(
                                Padding::new(structure.small_gap * 2.0)
                                    .left(button_size * 2.0 + structure.small_gap * 2.0)
                            )
                            .on_action(InputMessage::TextAction)
                            .key_binding(move |key| {
                                if !key.is_focused {
                                    return None;
                                }
                                if !key.modifiers.shift()
                                    && matches!(
                                        key.key,
                                        iced::keyboard::Key::Named(
                                            iced::keyboard::key::Named::Enter
                                        )
                                    )
                                {
                                    return Some(text_editor::Binding::Custom(
                                        InputMessage::SendMessage,
                                    ));
                                }

                                if matches!(
                                    key.key,
                                    iced::keyboard::Key::Named(
                                        iced::keyboard::key::Named::Backspace
                                    )
                                ) {
                                    return Some(text_editor::Binding::Custom(
                                        InputMessage::BackspacePressed {
                                            ctrl: key.modifiers.control(),
                                        },
                                    ));
                                }

                                let keybinds = keybinds.borrow();
                                let is_reserved =
                                    keybinds.action_for_key(&key.key, &key.modifiers).is_some();
                                drop(keybinds);

                                if is_reserved {
                                    return None;
                                }

                                match text_editor::Binding::from_key_press(key) {
                                    Some(text_editor::Binding::Paste) => {
                                        Some(text_editor::Binding::Custom(InputMessage::Paste))
                                    }
                                    binding => binding,
                                }
                            })
                            .wrapping(text::Wrapping::WordOrGlyph)
                            .line_height(text::LineHeight::Relative(1.0))
                            .height(Length::Fit.min(line_height).max(line_height * max_lines))
                            .style(move |_, status| w::text_editor::Style {
                                background: theme.solid_bg.into(),
                                border: Border {
                                    color: if status.active() {
                                        theme.accent.into()
                                    } else {
                                        theme.border.into()
                                    },
                                    width: 1.0,
                                    radius: Radius::from(structure.inner_border_radius).top(
                                        if !is_replying_to && !has_attachments {
                                            structure.inner_border_radius
                                        } else {
                                            0.0
                                        }
                                    ),
                                },
                                placeholder: theme.text.muted.into(),
                                selection: theme.accent.into(),
                                value: theme.text.normal.into()
                            }),
                        w::row![
                            input_button(
                                phosphor_svgs::icon::plus::BOLD,
                                HelpKey::Chat(ChatHelpKey::UploadButton),
                                "Upload button, press to upload files",
                                "Upload attachments",
                                InputMessage::OpenAttachmentPicker,
                            ),
                            input_button(
                                phosphor_svgs::icon::file_archive::BOLD,
                                HelpKey::Chat(ChatHelpKey::UploadCompressedButton),
                                "Upload compressed button, press to upload a compressed folder",
                                "Upload compressed folder",
                                InputMessage::OpenCompressedAttachmentPicker,
                            ),
                        ]
                        .spacing(-structure.small_gap * 2.0)
                    ]
                ]
                .width(Fill),
            )
            .radius(structure.inner_border_radius)
            .into()
    }
}
