use anyhow::Result;
use matrix_sdk_ui::{Timeline, timeline::AttachmentSource};
use std::io::Cursor;

use ego_tree::NodeRef;
use image::ImageReader;
use matrix_sdk::{
    Room,
    attachment::{
        AttachmentConfig, AttachmentInfo, BaseFileInfo, BaseImageInfo, BaseVideoInfo, Thumbnail,
    },
    room::{
        edit::EditedContent,
        reply::{EnforceThread, Reply},
    },
};
use mime_guess::mime;
use ruma::{
    OwnedEventId, OwnedTransactionId, OwnedUserId,
    events::{
        AnyMessageLikeEventContent, Mentions,
        room::message::{
            AddMentions, Relation, RoomMessageEventContent, RoomMessageEventContentWithoutRelation,
        },
    },
};
use scraper::{Html, Node};
use url::Url;

use super::attachments::MatrixAttachment;

pub trait RoomSendingExt {
    fn send_message(
        &self,
        html: String,
        replies_to: Option<OwnedEventId>,
    ) -> impl Future<Output = Result<()>>;
    fn send_deplace_attachment(
        &self,
        attachment: MatrixAttachment,
        replies_to: Option<OwnedEventId>,
        txn_id: Option<OwnedTransactionId>,
    ) -> impl Future<Output = Result<OwnedTransactionId>>;
    fn edit_message(&self, message: OwnedEventId, html: String)
    -> impl Future<Output = Result<()>>;
}

impl RoomSendingExt for Room {
    async fn send_message(&self, html: String, replies_to: Option<OwnedEventId>) -> Result<()> {
        let queue = self.send_queue();

        let mut mentions = Mentions::default();

        let (body, formatted_body, _urls) = process_string_to_message(&html, &mut mentions);

        if body.is_empty() || &body == "\n" {
            tracing::warn!("Body is empty, not committing message");
            return Ok(());
        }

        let mut message_content = if let Some(formatted_body) = formatted_body {
            RoomMessageEventContent::text_html(body, formatted_body)
        } else {
            RoomMessageEventContent::text_plain(body)
        };
        message_content.mentions = Some(mentions);

        if let Some(reply_to_id) = replies_to {
            message_content.relates_to = Some(Relation::Reply(
                matrix_sdk::ruma::events::relation::Reply::with_event_id(reply_to_id),
            ));
        }

        let content = AnyMessageLikeEventContent::RoomMessage(message_content);
        queue.send(content).await?;

        Ok(())
    }

    async fn send_deplace_attachment(
        &self,
        attachment: MatrixAttachment,
        replies_to: Option<OwnedEventId>,
        txn_id: Option<OwnedTransactionId>,
    ) -> Result<OwnedTransactionId> {
        let queue = self.send_queue();

        let size = attachment.data.len() as u32;
        // Decoding + blurhashing a large image is real CPU work; keep it off whatever
        // thread is driving this task, same as everywhere else that touches image data.
        let mime_type = attachment.mime_type.clone();
        let data = attachment.data.clone();
        let source_path = attachment.source_path.clone();
        let probe_start = std::time::Instant::now();
        let (info, thumbnail) = tokio::task::spawn_blocking(move || match mime_type.type_() {
            mime::IMAGE => {
                let img = ImageReader::new(Cursor::new(&data))
                    .with_guessed_format()
                    .ok()
                    .and_then(|r| r.decode().ok());

                let dimensions = img.as_ref().map(|i| (i.width(), i.height()));

                let bh = img.as_ref().and_then(|img| {
                    let thumb = img.thumbnail(64, 64);
                    let rgba = thumb.to_rgba8();
                    blurhash::encode(4, 3, rgba.width(), rgba.height(), &rgba).ok()
                });

                let info = BaseImageInfo {
                    width: dimensions.map(|(w, _)| w.into()),
                    height: dimensions.map(|(_, h)| h.into()),
                    size: Some(size.into()),
                    blurhash: bh,
                    is_animated: None,
                };

                (AttachmentInfo::Image(info), None)
            }
            mime::VIDEO => {
                let metadata =
                    super::video_info::probe_video(
                        &data,
                        source_path.as_deref(),
                        &format!(".{}", mime_type.subtype()),
                    );

                let thumbnail = metadata.thumbnail_jpeg.map(|jpeg| {
                    (
                        jpeg,
                        metadata.thumbnail_width.unwrap_or_default().into(),
                        metadata.thumbnail_height.unwrap_or_default().into(),
                    )
                });

                let info = BaseVideoInfo {
                    height: metadata.height.map(Into::into),
                    width: metadata.width.map(Into::into),
                    size: Some(size.into()),
                    blurhash: metadata.blurhash,
                    duration: metadata.duration,
                };

                (AttachmentInfo::Video(info), thumbnail)
            }
            _ => (
                AttachmentInfo::File(BaseFileInfo {
                    size: Some(size.into()),
                }),
                None,
            ),
        })
        .await?;

        tracing::info!(
            filename = %attachment.filename,
            size,
            probe_ms = probe_start.elapsed().as_millis() as u64,
            has_thumbnail = thumbnail.is_some(),
            "attachment metadata prepared"
        );

        let thumbnail = thumbnail.map(|(data, width, height)| Thumbnail {
            size: u32::try_from(data.len()).unwrap_or(u32::MAX).into(),
            data,
            content_type: mime::IMAGE_JPEG,
            width,
            height,
        });

        let txn_id = txn_id.unwrap_or_else(ruma::TransactionId::new);

        let config = AttachmentConfig {
            txn_id: Some(txn_id.clone()),
            info: Some(info),
            thumbnail,
            caption: None,
            reply: replies_to.map(|event_id| Reply {
                event_id,
                enforce_thread: EnforceThread::MaybeThreaded,
                add_mentions: AddMentions::Yes,
            }),
            mentions: None,
            extra_content: None,
        };

        let queue_start = std::time::Instant::now();
        let filename = attachment.filename.clone();
        queue
            .send_attachment(
                attachment.filename,
                attachment.mime_type,
                attachment.data.into(),
                config,
            )
            .await?;
        tracing::info!(
            filename = %filename,
            size,
            queue_ms = queue_start.elapsed().as_millis() as u64,
            "attachment handed to send queue"
        );

        Ok(txn_id)
    }

    async fn edit_message(&self, message: OwnedEventId, html: String) -> Result<()> {
        let queue = self.send_queue();

        if html.is_empty() || &html == "\n" {
            tracing::warn!("Body is empty, not committing message");
            return Ok(());
        }

        let body =
            EditedContent::RoomMessage(RoomMessageEventContentWithoutRelation::text_plain(html));

        let event = self.make_edit_event(&message, body).await?;

        queue.send(event).await?;
        Ok(())
    }
}

impl RoomSendingExt for Timeline {
    async fn send_message(&self, html: String, replies_to: Option<OwnedEventId>) -> Result<()> {
        let mut mentions = Mentions::default();

        let (body, formatted_body, _urls) = process_string_to_message(&html, &mut mentions);

        if body.is_empty() || &body == "\n" {
            tracing::warn!("Body is empty, not committing message");
            return Ok(());
        }

        let mut message_content = if let Some(formatted_body) = formatted_body {
            RoomMessageEventContent::text_html(body, formatted_body)
        } else {
            RoomMessageEventContent::text_plain(body)
        };
        message_content.mentions = Some(mentions);

        if let Some(reply_to_id) = replies_to {
            message_content.relates_to = Some(Relation::Reply(
                matrix_sdk::ruma::events::relation::Reply::with_event_id(reply_to_id),
            ));
        }

        let content = AnyMessageLikeEventContent::RoomMessage(message_content);
        self.send(content).await?;

        Ok(())
    }

    async fn send_deplace_attachment(
        &self,
        attachment: MatrixAttachment,
        replies_to: Option<OwnedEventId>,
        txn_id: Option<OwnedTransactionId>,
    ) -> Result<OwnedTransactionId> {
        let size = attachment.data.len() as u32;
        let mime_type = attachment.mime_type.clone();
        let data = attachment.data.clone();
        let source_path = attachment.source_path.clone();
        let probe_start = std::time::Instant::now();
        let (info, thumbnail) = tokio::task::spawn_blocking(move || match mime_type.type_() {
            mime::IMAGE => {
                let img = ImageReader::new(Cursor::new(&data))
                    .with_guessed_format()
                    .ok()
                    .and_then(|r| r.decode().ok());

                let dimensions = img.as_ref().map(|i| (i.width(), i.height()));

                let bh = img.as_ref().and_then(|img| {
                    let thumb = img.thumbnail(64, 64);
                    let rgba = thumb.to_rgba8();
                    blurhash::encode(4, 3, rgba.width(), rgba.height(), &rgba).ok()
                });

                let info = BaseImageInfo {
                    width: dimensions.map(|(w, _)| w.into()),
                    height: dimensions.map(|(_, h)| h.into()),
                    size: Some(size.into()),
                    blurhash: bh,
                    is_animated: None,
                };

                (AttachmentInfo::Image(info), None)
            }
            mime::VIDEO => {
                let metadata =
                    super::video_info::probe_video(
                        &data,
                        source_path.as_deref(),
                        &format!(".{}", mime_type.subtype()),
                    );

                let thumbnail = metadata.thumbnail_jpeg.map(|jpeg| {
                    (
                        jpeg,
                        metadata.thumbnail_width.unwrap_or_default().into(),
                        metadata.thumbnail_height.unwrap_or_default().into(),
                    )
                });

                let info = BaseVideoInfo {
                    height: metadata.height.map(Into::into),
                    width: metadata.width.map(Into::into),
                    size: Some(size.into()),
                    blurhash: metadata.blurhash,
                    duration: metadata.duration,
                };

                (AttachmentInfo::Video(info), thumbnail)
            }
            _ => (
                AttachmentInfo::File(BaseFileInfo {
                    size: Some(size.into()),
                }),
                None,
            ),
        })
        .await?;

        tracing::info!(
            filename = %attachment.filename,
            size,
            probe_ms = probe_start.elapsed().as_millis() as u64,
            has_thumbnail = thumbnail.is_some(),
            "attachment metadata prepared"
        );

        let thumbnail = thumbnail.map(|(data, width, height)| Thumbnail {
            size: u32::try_from(data.len()).unwrap_or(u32::MAX).into(),
            data,
            content_type: mime::IMAGE_JPEG,
            width,
            height,
        });

        let txn_id = txn_id.unwrap_or_else(ruma::TransactionId::new);

        let config = matrix_sdk_ui::timeline::AttachmentConfig {
            txn_id: Some(txn_id.clone()),
            info: Some(info),
            thumbnail,
            caption: None,
            in_reply_to: replies_to,
            mentions: None,
            extra_content: None,
        };

        tracing::debug!(filename = %attachment.filename, size, "queuing attachment for upload");

        // Media upload progress must be opted in client-wide.
        self.room()
            .client()
            .send_queue()
            .enable_upload_progress(true);

        let queue_start = std::time::Instant::now();
        self.send_attachment(
            AttachmentSource::Data {
                bytes: attachment.data.into(),
                filename: attachment.filename,
            },
            attachment.mime_type,
            config,
        )
        .use_send_queue()
        .await?;

        tracing::info!(
            size,
            queue_ms = queue_start.elapsed().as_millis() as u64,
            "attachment queued successfully"
        );

        Ok(txn_id)
    }

    async fn edit_message(&self, message: OwnedEventId, html: String) -> Result<()> {
        if html.is_empty() || &html == "\n" {
            tracing::warn!("Body is empty, not committing message");
            return Ok(());
        }

        let body =
            EditedContent::RoomMessage(RoomMessageEventContentWithoutRelation::text_plain(html));

        self.edit(
            &matrix_sdk_ui::timeline::TimelineEventItemId::EventId(message),
            body,
        )
        .await?;

        Ok(())
    }
}

fn process_string_to_message(
    html: &str,
    mentions: &mut Mentions,
) -> (String, Option<String>, Vec<Url>) {
    let fragment = Html::parse_fragment(html);

    let mut body = String::new();
    let mut formatted_body = String::new();
    let mut urls = Vec::new();

    for node in fragment.tree.root().children() {
        walk_node(node, mentions, &mut body, &mut formatted_body, &mut urls);
    }

    let formatted_body = (formatted_body != body).then_some(formatted_body);

    (body, formatted_body, urls)
}

fn walk_node(
    node: NodeRef<'_, Node>,
    mentions: &mut Mentions,
    body: &mut String,
    formatted: &mut String,
    urls: &mut Vec<Url>,
) {
    match node.value() {
        Node::Text(text) => {
            let content = text.text.replace("\u{a0}", " ");
            body.push_str(&content);
            formatted.push_str(&content);
        }
        Node::Element(elem) => {
            if let Some(url) = elem.attr("data-url") {
                let display_text = extract_text(node);

                body.push_str(&display_text);
                formatted.push_str(&format!("<a href=\"{}\">{}</a>", url, display_text));
                if let Ok(parsed) = Url::parse(url) {
                    urls.push(parsed);
                }
                return;
            }
            if let Some(data_type) = elem.attr("data-type")
                && let Some(id) = elem.attr("data-id")
            {
                let display_text = extract_text(node).trim_start_matches('@').to_string();

                if data_type == "room_mention" {
                    body.push_str("#room");
                    mentions.room = true;
                    formatted.push_str(&format!(
                        "<a href=\"https://matrix.to/#/{}\">{}</a>",
                        id, display_text
                    ));
                } else if data_type == "user_mention" {
                    if let Ok(user_id) = OwnedUserId::try_from(id) {
                        mentions.user_ids.insert(user_id);
                    } else {
                        tracing::warn!("Invalid user ID in mention: {id}");
                    }

                    body.push_str(&display_text);
                    formatted.push_str(&format!(
                        "<a href=\"https://matrix.to/#/{}\">{}</a>",
                        id, display_text
                    ));
                }
                return;
            }

            match elem.name() {
                "html" | "body" => {
                    for child in node.children() {
                        walk_node(child, mentions, body, formatted, urls);
                    }
                }
                "br" => {
                    body.push('\n');
                    formatted.push_str("<br>");
                }
                other => tracing::warn!("Unknown element: {other}; {:?}", elem),
            }
        }
        _ => {}
    }
}

fn extract_text(node: NodeRef<'_, Node>) -> String {
    let mut text = String::new();
    for child in node.children() {
        if let Node::Text(t) = child.value() {
            text.push_str(&t.text);
        } else {
            text.push_str(&extract_text(child));
        }
    }
    text
}
