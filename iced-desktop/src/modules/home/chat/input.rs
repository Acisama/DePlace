use deplace_core::{
    keybinds::Keybinds,
    matrix_api::{attachments::MatrixAttachment, messages::RoomSendingExt},
};
use iced::{
    Alignment, Length,
    border::{Radius, radius},
    futures::SinkExt,
    widget::{Id, operation, text::LineHeight, text_editor},
};
use image::ImageReader;
use macros::{iced_cache, iced_icon};
use matrix_sdk::send_queue::RoomSendQueueUpdate;
use matrix_sdk_ui::Timeline;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    path::PathBuf,
};
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use crate::{
    common::*,
    components::{InsetShadow, help_mode::ChatHelpKey, phosphor_icon},
};

use super::{ChatMessage, timeline::messages::MessageEvent};

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
    /// Set while the bytes are still being encoded (a clipboard image, say):
    /// the chip already shows, but sending waits for this to clear.
    pending: bool,
    /// Set while the attachment is in the send queue, so its chip can show
    /// upload progress and leave once the upload finished.
    upload: Option<UploadState>,
}

#[derive(Debug, Clone, Copy)]
struct UploadState {
    current: usize,
    total: usize,
}

impl Hash for Attachment {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.pending.hash(state);
        self.upload.map(|u| (u.current, u.total)).hash(state);
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
            upload: None,
        }
    }
}

/// Longest side of a chip preview: decoding at full size stalls the window,
/// because the texture upload runs on the render thread in `view()`.
const ATTACHMENT_THUMBNAIL_SIZE: u32 = 256;

fn generate_thumbnail(data: &[u8], mime_type: &mime::Mime) -> Option<ImageHandle> {
    // Videos are probed in the background and at send time, so decoding one
    // here would hold the chip back for seconds.
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

/// The same downscale as [`generate_thumbnail`], for raw pixels off the clipboard.
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

/// Gives every attachment its own blocking task, so thumbnails decode in
/// parallel and each chip appears as soon as its own file is ready.
fn attachments_task(attachments: Vec<MatrixAttachment>) -> Task<InputMessage> {
    Task::batch(attachments.into_iter().map(|attachment| {
        Task::future(async move {
            match tokio::task::spawn_blocking(move || attachment.into()).await {
                Ok(attachment) => Some(attachment),
                Err(e) => {
                    tracing::error!("Failed to build attachment: {}", e);
                    None
                }
            }
        })
        .then(|built| {
            let preview = built
                .as_ref()
                .map(video_preview_task)
                .unwrap_or_else(|| Task::done(InputMessage::None));
            let added = Task::done(InputMessage::AttachmentAdded(
                built.map(|a| vec![Arc::new(a)]).unwrap_or_default(),
            ));
            Task::batch([added, preview])
        })
    }))
}

/// Pastes the clipboard image as a PNG attachment.
///
/// The chip shows up at once with a downscaled preview and the full-size encode
/// follows; with no image on the clipboard the text fallback is pasted.
fn image_paste_task(fallback: Option<Arc<String>>) -> Task<InputMessage> {
    iced::clipboard::read_image().then(move |result| match result {
        Ok(image) => {
            let id = Uuid::now_v7();

            // Downscaling still goes through spawn_blocking so large images
            // cause no freezes
            let show_now = Task::future({
                let image = image.clone();
                async move {
                    let thumbnail = tokio::task::spawn_blocking(move || {
                        downscale_thumbnail(image.size.width, image.size.height, &image.rgba)
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
                        upload: None,
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
        // Nothing on the clipboard but maybe text so paste it.
        Err(_) => fallback
            .clone()
            .map(|text| {
                Task::done(InputMessage::TextAction(text_editor::Action::Edit(
                    text_editor::Edit::Paste(text),
                )))
            })
            .unwrap_or_else(|| Task::done(InputMessage::None)),
    })
}

/// Swaps a video chip's extension badge for a decoded frame once probing
/// finishes, the chip itself shows immediately, badge and all.
fn video_preview_task(attachment: &Attachment) -> Task<InputMessage> {
    if attachment.attachment.mime_type.type_() != mime::VIDEO {
        return Task::done(InputMessage::None);
    }

    let id = attachment.id;
    let data = attachment.attachment.data.clone();
    let source_path = attachment.attachment.source_path.clone();
    let mime_type = attachment.attachment.mime_type.clone();

    Task::future(async move {
        let probe_start = std::time::Instant::now();
        let handle = tokio::task::spawn_blocking(move || {
            deplace_core::matrix_api::video_info::probe_video(
                &data,
                source_path.as_deref(),
                &format!(".{}", mime_type.subtype()),
            )
            .thumbnail_jpeg
            .and_then(|jpeg| {
                ImageReader::new(Cursor::new(jpeg))
                    .decode()
                    .ok()
                    .map(|img| {
                        img.thumbnail(ATTACHMENT_THUMBNAIL_SIZE, ATTACHMENT_THUMBNAIL_SIZE)
                            .to_rgba8()
                    })
            })
            .map(|img| ImageHandle::from_rgba(img.width(), img.height(), img.into_raw()))
        })
        .await
        .ok()
        .flatten();

        tracing::info!(
            id = %id,
            probe_ms = probe_start.elapsed().as_millis() as u64,
            has_preview = handle.is_some(),
            "video preview probed"
        );

        match handle {
            Some(handle) => InputMessage::AttachmentPreview(id, handle),
            None => InputMessage::None,
        }
    })
}

fn attachment_task(attachment: Option<MatrixAttachment>) -> Task<InputMessage> {
    Task::future(async move {
        match attachment {
            Some(attachment) => {
                match tokio::task::spawn_blocking(move || attachment.into()).await {
                    Ok(attachment) => Some(attachment),
                    Err(e) => {
                        tracing::error!("Failed to build attachment: {}", e);
                        None
                    }
                }
            }
            None => None,
        }
    })
    .then(|built| {
        let preview = built
            .as_ref()
            .map(video_preview_task)
            .unwrap_or_else(|| Task::done(InputMessage::None));
        let added = Task::done(InputMessage::AttachmentAdded(
            built.map(|a| vec![Arc::new(a)]).unwrap_or_default(),
        ));
        Task::batch([added, preview])
    })
}

/// Queues one attachment and streams its upload progress into the input bar.
///
/// Subscribing happens before queueing, so a tiny upload cannot finish in
/// between and leave its chip stuck forever.
fn attachment_upload_task(
    timeline: Arc<Timeline>,
    id: Uuid,
    attachment: MatrixAttachment,
    replies_to: Option<OwnedEventId>,
) -> Task<InputMessage> {
    Task::stream(iced::stream::channel::<InputMessage>(
        64,
        move |mut output: iced::futures::channel::mpsc::Sender<InputMessage>| async move {
            macro_rules! emit {
                ($msg:expr) => {
                    if output.send($msg).await.is_err() {
                        return;
                    }
                };
            }

            let queue = timeline.room().send_queue();

            let started = std::time::Instant::now();
            let (_echoes, mut rx) = match queue.subscribe().await {
                Ok(subscribed) => subscribed,
                Err(e) => {
                    tracing::warn!("Failed to subscribe to send queue: {}", e);
                    emit!(InputMessage::UploadFailed(id));
                    return;
                }
            };

            let txn_id = match timeline
                .send_deplace_attachment(attachment, replies_to, None)
                .await
            {
                Ok(txn_id) => txn_id,
                Err(e) => {
                    tracing::warn!("Failed to queue attachment: {}", e);
                    emit!(InputMessage::UploadFailed(id));
                    return;
                }
            };
            tracing::info!(
                id = %id,
                queue_ms = started.elapsed().as_millis() as u64,
                "attachment queued, waiting for upload progress"
            );

            loop {
                match rx.recv().await {
                    Ok(RoomSendQueueUpdate::MediaUpload {
                        related_to,
                        progress,
                        ..
                    }) if related_to == txn_id => {
                        tracing::info!(
                            id = %id,
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            current = progress.current,
                            total = progress.total,
                            "media upload progress"
                        );
                        emit!(InputMessage::UploadProgress(
                            id,
                            progress.current,
                            progress.total,
                        ));

                        // The send queue uploads first and sends the event right
                        // after, so full progress means the chip can leave now.
                        if progress.total > 0 && progress.current >= progress.total {
                            emit!(InputMessage::UploadDone(id));
                            return;
                        }
                    }
                    Ok(RoomSendQueueUpdate::SentEvent { transaction_id, .. })
                        if transaction_id == txn_id =>
                    {
                        tracing::info!(
                            id = %id,
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "attachment upload sent"
                        );
                        emit!(InputMessage::UploadDone(id));
                        return;
                    }
                    Ok(RoomSendQueueUpdate::SendError {
                        transaction_id,
                        error,
                        ..
                    }) if transaction_id == txn_id => {
                        tracing::warn!(
                            id = %id,
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "Attachment upload failed: {}",
                            error
                        );
                        emit!(InputMessage::UploadFailed(id));
                        return;
                    }
                    Ok(_) => {}
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!("Missed {n} send queue updates for an upload");
                        // Keep going; only completion matters now, and progress
                        // will be corrected by the next MediaUpload update.
                    }
                    Err(_) => {
                        emit!(InputMessage::UploadFailed(id));
                        return;
                    }
                }
            }
        },
    ))
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
    AttachmentPreview(Uuid, ImageHandle),
    Paste,
    AttachmentReady(Uuid, MatrixAttachment),
    AttachmentFailed(Uuid),
    UploadProgress(Uuid, usize, usize),
    UploadDone(Uuid),
    UploadFailed(Uuid),
    RemoveAttachment(usize),
}

impl HelpOverMessage for InputMessage {
    fn help_over(help: Option<HelpKey>) -> Self {
        InputMessage::HelpOver(help)
    }
}

pub enum InputAction {
    /// The send was accepted. The task yields follow-up messages (upload
    /// progress, ...) that must be fed back into this widget.
    SendMessage(Task<ChatMessage>),
    RemoveReplying,
    HelpOver(Option<HelpKey>),
    Perform(Task<InputMessage>),
}

#[iced_cache(Clone)]
pub struct ChatInput {
    state: AppState,
    pub timeline: Option<Arc<Timeline>>,

    room: DePlaceRoom,

    id: Id,

    #[hash]
    hovered_files: BTreeSet<PathBuf>,

    /// The `[crate::components::IcedWidget]` crate requires 'static lifetimes for caching, but `text_editor` requires a reference to the content since it stores the data internally.
    content: *mut text_editor::Content,
    #[hash]
    replying_to: Option<(Arc<MessageEvent>, OwnedEventId)>,

    #[hash]
    attachments: Vec<Attachment>,

    /// An outcome that arrived before its chip, held until the chip shows up.
    ready_attachments: BTreeMap<Uuid, MatrixAttachment>,
    failed_attachments: BTreeSet<Uuid>,

    keybinds: Receiver<Keybinds>,
}

impl ExtraHash for ChatInput {
    fn extra_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.room.hash_selective(state, true, false, false, false);
    }
}

impl ChatInput {
    pub fn new(state: &AppState, room: &DePlaceRoom) -> Self {
        Self {
            state: state.clone(),
            timeline: None,

            room: room.clone(),

            hovered_files: BTreeSet::new(),

            id: Id::unique(),
            content: Box::leak(Box::new(text_editor::Content::new())),
            replying_to: None,

            attachments: Vec::new(),

            ready_attachments: BTreeMap::new(),
            failed_attachments: BTreeSet::new(),

            keybinds: state.keybinds(),
        }
    }

    pub fn add_hovered_file(&mut self, path: PathBuf) {
        self.hovered_files.insert(path);
    }

    pub fn clear_hovered_files(&mut self) {
        self.hovered_files.clear();
    }

    pub fn commit_hovered_file(&mut self, path: PathBuf) -> Task<InputMessage> {
        self.hovered_files.remove(&path);
        let state = self.state.clone();
        Task::future(async move {
            let display_path = path.clone();
            match MatrixAttachment::from_path(path, state).await {
                Ok(attachment) => Some(attachment),
                Err(e) => {
                    tracing::error!("Failed to attach {}: {e}", display_path.display());
                    None
                }
            }
        })
        .then(attachment_task)
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
                for mut attachment in attachments {
                    if self.failed_attachments.remove(&attachment.id) {
                        tracing::debug!(
                            "Dropping attachment {}: it failed to encode before its preview was added",
                            attachment.id
                        );
                        continue;
                    }

                    if let Some(ready) = self.ready_attachments.remove(&attachment.id) {
                        tracing::debug!(
                            "Attachment {} finished encoding before its preview arrived, completing it now",
                            attachment.id
                        );
                        let thumbnail = attachment.thumbnail.clone();
                        attachment = Arc::new(Attachment {
                            id: attachment.id,
                            thumbnail,
                            attachment: ready,
                            pending: false,
                            upload: None,
                        });
                    }

                    self.attachments.push(Arc::unwrap_or_clone(attachment));
                }
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
                // Paste order: text carrying file:// URIs, then files, then an
                // image, and plain text only when there is nothing else.
                tracing::debug!("Paste: starting the clipboard chain");
                let task = iced::clipboard::read_text().then(|result| {
                    tracing::debug!(?result, "Paste: text read");
                    let text = result.ok().filter(|text| !text.is_empty());
                    match text {
                        Some(ref text) if text.contains("file://") => {
                            // File managers copy files as file:// URIs rather
                            // than text, e.g. a video pasted from a file browser.
                            tracing::debug!("Paste: text carries file URIs, attaching");
                            Task::future(MatrixAttachment::from_clipboard_file_list(
                                text.to_string(),
                            ))
                            .then(attachments_task)
                        }
                        text => iced::clipboard::read_files().then(move |result| {
                            tracing::debug!(?result, "Paste: file list read");
                            match result {
                                Ok(files) if !files.is_empty() => {
                                    tracing::debug!(count = files.len(), "Paste: attaching files",);
                                    Task::future(MatrixAttachment::from_clipboard_file_list(
                                        files
                                            .iter()
                                            .map(|path| path.to_string_lossy().into_owned())
                                            .collect::<Vec<_>>()
                                            .join("\n"),
                                    ))
                                    .then(attachments_task)
                                }
                                // No file list: image paste with the text as
                                // the last-resort fallback.
                                _ => {
                                    tracing::debug!("Paste: falling back to the image read");
                                    image_paste_task(text.clone())
                                }
                            }
                        }),
                    }
                });

                Some(InputAction::Perform(task))
            }
            InputMessage::AttachmentReady(id, attachment) => {
                if let Some(existing) = self.attachments.iter_mut().find(|a| a.id == id) {
                    *existing = Attachment {
                        id,
                        thumbnail: existing.thumbnail.clone(),
                        attachment,
                        pending: false,
                        upload: None,
                    };
                } else {
                    // hold the result until the matching `AttachmentAdded`
                    // arrives.
                    self.ready_attachments.insert(id, attachment);
                }
                None
            }
            InputMessage::AttachmentFailed(id) => {
                if !self.attachments.iter().any(|a| a.id == id) {
                    // pending attachment added afterwards must be dropped too.
                    self.failed_attachments.insert(id);
                }
                self.attachments.retain(|a| a.id != id);
                None
            }
            InputMessage::UploadProgress(id, current, total) => {
                if let Some(attachment) = self.attachments.iter_mut().find(|a| a.id == id) {
                    // Progress never goes backwards (e.g. a thumbnail whose
                    // bytes land after the file finished).
                    let previous = match attachment.upload {
                        Some(state) => state.current,
                        None => 0,
                    };
                    attachment.upload = Some(UploadState {
                        current: current.max(previous),
                        total,
                    });
                }
                None
            }
            InputMessage::UploadDone(id) => {
                self.attachments.retain(|a| a.id != id);
                Some(InputAction::Perform(self.focus()))
            }
            InputMessage::UploadFailed(id) => {
                // Keep the chip so the user can retry with another Enter.
                if let Some(attachment) = self.attachments.iter_mut().find(|a| a.id == id) {
                    attachment.upload = None;
                }
                None
            }
            InputMessage::AttachmentPreview(id, handle) => {
                for attachment in &mut self.attachments {
                    if attachment.id == id
                        && matches!(attachment.thumbnail, ThumbnailType::Extention(_))
                    {
                        attachment.thumbnail = ThumbnailType::Image(handle.clone());
                        break;
                    }
                }
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
                        self.room.room_id()
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

                // Encoding attachments stay, uploading ones are in the send
                // queue, the rest are queued here and keep their chips until done.
                let mut upload_tasks = Vec::new();
                // Only the first outgoing item carries the
                // reply, same as before.
                let mut first_reply = text.is_none().then(|| replying_to.clone()).flatten();
                self.attachments.retain_mut(|attachment| {
                    if attachment.pending || attachment.upload.is_some() {
                        return true;
                    }

                    let total = attachment.attachment.data.len();
                    let id = attachment.id;
                    let to_send = attachment.attachment.clone();
                    let replies_to = first_reply.take();
                    attachment.upload = Some(UploadState { current: 0, total });

                    upload_tasks.push(attachment_upload_task(
                        timeline.clone(),
                        id,
                        to_send,
                        replies_to,
                    ));
                    true
                });

                let text_task = Task::future({
                    let timeline = timeline.clone();
                    async move {
                        if let Some(text) = text {
                            if let Err(e) = timeline.send_message(text, replying_to).await {
                                tracing::warn!("Failed to send message: {}", e);
                            }
                        }
                    }
                });

                Some(InputAction::SendMessage(Task::batch([
                    text_task.map(|()| ChatMessage::None),
                    Task::batch(upload_tasks).map(ChatMessage::Input),
                ])))
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

    fn view(&self) -> Element<'static, InputMessage> {
        let theme = theme();
        let structure = structure();
        let help_state = help_state();

        let keybinds = self.keybinds.clone();
        let line_height = structure.chat.text_size;

        let button_size = line_height + structure.small_gap * 2.0;

        let reply_bar_size = structure.chat.text_size + structure.small_gap * 2.0;

        let help_view = HelpView::new(theme);

        let input_button = |svg: &'static str, key, help, hover_tip, message| {
            w::container(
                help_view
                    .call(
                        key,
                        help,
                        themed_tooltip_text(
                            animated_button(phosphor_icon(svg, line_height))
                                .style(move |status| ButtonStyle {
                                    text_color: if status.active() {
                                        theme.text.normal.into()
                                    } else {
                                        theme.text.dim.into()
                                    },
                                    background: status
                                        .active()
                                        .then_some(theme.solid_hover_bg.into()),
                                    border: border::rounded(structure.semi_large_border_radius()),
                                    ..Default::default()
                                })
                                .on_press(message)
                                .padding(structure.small_gap),
                            hover_tip,
                            structure,
                            theme,
                        ),
                    )
                    .radius(structure.semi_large_border_radius()),
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
        let has_attachments = !self.attachments.is_empty() || !self.hovered_files.is_empty();

        let attachment_banner: Option<Element<'static, InputMessage>> =
            has_attachments.then(|| {
                let size = structure.chat.attachment_preview_size;

                let children = self
                    .attachments
                    .iter()
                    .enumerate()
                    .map(|(index, attachment)| {
                        let image: Element<'static, InputMessage> = match attachment
                            .thumbnail
                            .clone()
                        {
                            ThumbnailType::Image(handle) => w::image(handle)
                                .width(size)
                                .height(size)
                                .border_radius(structure.semi_large_border_radius())
                                .boxed(),
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
                                                radius: structure.semi_large_border_radius().into(),
                                            },
                                            ..Default::default()
                                        }
                                    }),
                                    Canvas::new(InsetShadow::new(
                                        structure.semi_large_border_radius(),
                                        color,
                                        size / 8.0,
                                        16
                                    ))
                                    .width(size)
                                    .height(size),
                                ]
                                .clip(true)
                                .boxed()
                            }
                        };

                        // While uploading, dim the chip and pin an upload
                        // progress bar to its bottom edge.
                        let image: Element<'static, InputMessage> = match attachment.upload {
                            Some(upload) => {
                                let ratio = if upload.total > 0 {
                                    (upload.current as f32 / upload.total as f32).clamp(0.0, 1.0)
                                } else {
                                    0.0
                                };

                                w::stack![
                                    image,
                                    w::container(
                                        w::progress_bar(0.0..=1.0, ratio).girth(4.0).style(
                                            move |_| {
                                                w::progress_bar::Style {
                                                    background: theme.solid_bg.to_iced().into(),
                                                    bar: theme.accent.to_iced().into(),
                                                    border: Border::default(),
                                                }
                                            }
                                        ),
                                    )
                                    .width(size)
                                    .height(size)
                                    .padding(iced::Padding {
                                        top: 0.0,
                                        right: 4.0,
                                        bottom: 4.0,
                                        left: 4.0,
                                    })
                                    .align_y(iced::alignment::Vertical::Bottom)
                                    .style(move |_| {
                                        w::container::Style {
                                            background: Some(
                                                theme.solid_bg.set_alpha(0.75).to_iced().into(),
                                            ),
                                            border: Border {
                                                radius: structure.semi_large_border_radius().into(),
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        }
                                    }),
                                ]
                                .clip(true)
                                .width(size)
                                .height(size)
                                .boxed()
                            }
                            None => image,
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
                                        .color(theme.text.normal)
                                        .line_height(LineHeight::Relative(1.0)))
                                    .width(size)
                                ]
                                .spacing(structure.small_gap / 2.0),
                            )
                            .radius(radius(structure.semi_large_border_radius()).bottom(0.0))
                            .boxed()
                    });

                help_view
                    .call(
                        HelpKey::Chat(ChatHelpKey::Attachments),
                        "Attachments for the current message",
                        w::container(themed_scrollable(
                            w::Row::with_children(children)
                                .width(Fill)
                                .height(size + structure.chat.text_size + 2.5 * structure.small_gap)
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
                    .boxed()
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
                            close_button(InputMessage::RemoveReplying)
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
                                self.room.room_id().to_owned(),
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
                .boxed()
        });

        let content = help_view
            .call(
                HelpKey::Chat(ChatHelpKey::Input),
                "The chat input, you can write messages here",
                w::column![
                    attachment_banner,
                    replying_banner,
                    w::stack![
                        w::text_editor(self.content())
                            .id(self.id.clone())
                            .placeholder(format!("Message {}", self.room.get_input_placeholder()))
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
            .boxed();

        if self.hovered_files.is_empty() {
            content
        } else {
            w::stack![
                content,
                w::container(
                    w::column![
                        iced_icon!(
                            upload_simple,
                            BOLD,
                            structure.chat.text_size * 2.0,
                            theme.accent
                        ),
                        weighted_text("Drop to upload", Weight::Bold)
                            .size(structure.chat.text_size * 1.3)
                            .color(theme.accent)
                    ]
                    .align_x(Alignment::Center)
                )
                .colored_rounded_with_border(
                    theme.accent.scale_alpha(0.3),
                    structure.inner_border_radius,
                    structure.divider_width,
                    theme.accent
                )
                .width(Fill)
                .height(Fill)
                .center(Fill)
            ]
            .boxed()
        }
    }
}
