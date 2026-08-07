use anyhow::Context;
use ego_tree::NodeRef;
use matrix_sdk::Client;
use ruma::{
    OwnedEventId, OwnedRoomId, OwnedUserId,
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

pub async fn send_message(
    html: String,
    matrix_client: Client,
    timeline_manager: TimelineManager,
    room_id: OwnedRoomId,
    replies_to: Option<OwnedEventId>,
) -> anyhow::Result<()> {
    tracing::debug!("Sending message to room {}", room_id);
    let room = matrix_client.get_room(&room_id).context("Room not found")?;

    let (timeline, _) = timeline_manager
        .get_or_create_timeline(
            &room,
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
