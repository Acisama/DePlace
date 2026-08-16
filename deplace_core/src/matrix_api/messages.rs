use std::io::Cursor;

use ego_tree::NodeRef;
use image::ImageReader;
use matrix_sdk::{
    Room,
    attachment::{AttachmentInfo, BaseFileInfo, BaseImageInfo, BaseVideoInfo},
};
use matrix_sdk_ui::timeline::{AttachmentConfig, AttachmentSource};
use mime_guess::{Mime, mime};
use ruma::{
    OwnedEventId, OwnedUserId,
    events::{
        AnyMessageLikeEventContent, Mentions,
        message::{MessageEventContent, MessageEventContentWithoutRelation},
        relation::Reply,
        room::message::{Relation, RoomMessageEventContent},
    },
};
use scraper::{Html, Node};
use url::Url;

use crate::matrix_api::timeline::TimelineManager;

#[derive(Clone)]
pub struct MatrixAttachment {
    pub filename: String,
    pub mime_type: Mime,
    pub data: Vec<u8>,
}

impl TimelineManager {
    pub async fn send_message(
        &self,
        html: String,
        room: &Room,
        replies_to: Option<OwnedEventId>,
    ) -> anyhow::Result<()> {
        tracing::debug!("Sending message to room {}", room.room_id());
        let (timeline, _) = self
            .get_or_create_timeline(
                room,
                matrix_sdk_ui::timeline::TimelineFocus::Live {
                    hide_threaded_events: false,
                },
            )
            .await?;

        let mut mentions = Mentions::default();

        let (body, formatted_body, _urls) = process_string_to_message(&html, &mut mentions);

        if body.is_empty() || &body == "\n" {
            tracing::warn!("Body is empty, not committing message");
            return Ok(());
        }

        if let Some(reply_to_id) = replies_to {
            let content: MessageEventContentWithoutRelation =
                if let Some(formatted_body) = formatted_body {
                    MessageEventContent::html(body, formatted_body).into()
                } else {
                    MessageEventContent::plain(body).into()
                };

            // content.url_previews = get_link_previews(&client, &urls).await;

            let content =
                content.with_relation(Some(Relation::Reply(Reply::with_event_id(reply_to_id))));
            timeline.send(content.into()).await?;
        } else {
            let mut message_content = if let Some(formatted_body) = formatted_body {
                RoomMessageEventContent::text_html(body, formatted_body)
            } else {
                RoomMessageEventContent::text_plain(body)
            };
            message_content.mentions = Some(mentions.clone());

            let content = AnyMessageLikeEventContent::RoomMessage(message_content);
            timeline.send(content).await?;
        }

        Ok(())
    }

    pub async fn send_attachment(
        &self,
        room: &Room,
        attachment: MatrixAttachment,
        replies_to: Option<OwnedEventId>,
    ) -> anyhow::Result<()> {
        tracing::debug!("Sending attachment to room {}", room.room_id());

        let (timeline, _) = self
            .get_or_create_timeline(
                room,
                matrix_sdk_ui::timeline::TimelineFocus::Live {
                    hide_threaded_events: false,
                },
            )
            .await?;

        let size = attachment.data.len() as u32;

        let info = match attachment.mime_type.subtype() {
            mime::IMAGE => {
                let img = ImageReader::new(Cursor::new(&attachment.data))
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

                AttachmentInfo::Image(info)
            }
            mime::VIDEO => {
                let info = BaseVideoInfo {
                    height: None,
                    width: None,
                    size: None,
                    blurhash: None,
                    duration: None,
                };

                AttachmentInfo::Video(info)
            }
            _ => AttachmentInfo::File(BaseFileInfo {
                size: Some(size.into()),
            }),
        };

        let config = AttachmentConfig {
            txn_id: None,
            info: Some(info),
            thumbnail: None,
            caption: None,
            in_reply_to: replies_to,
            mentions: None,
        };

        timeline
            .send_attachment(
                AttachmentSource::Data {
                    bytes: attachment.data,
                    filename: attachment.filename,
                },
                attachment.mime_type,
                config,
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
