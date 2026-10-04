//! Parsing of the `formatted_body` (HTML, as suggested by the Matrix specification) of a message
//! into a structure that is convenient to render.

use std::hash::Hash;

use ego_tree::NodeRef;
use ruma::{
    OwnedEventId, OwnedRoomAliasId, OwnedRoomId, OwnedRoomOrAliasId, OwnedUserId,
    matrix_uri::{MatrixId, MatrixToUri, MatrixUri},
};
use scraper::{Html, Node};

use crate::colors::DePlaceColor;

/// A Matrix message body that has been parsed from its `formatted_body` HTML representation.
#[derive(Debug, Clone, PartialEq, Default, Hash)]
pub struct FormattedBody {
    pub blocks: Vec<Block>,
}

/// A block-level element of a [`FormattedBody`].
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    BlockQuote(Vec<Block>),
    List {
        ordered: bool,
        start: Option<i64>,
        items: Vec<Vec<Block>>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    ThematicBreak,
}

/// An inline element within a [`Block`].
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Inline {
    Text(TextRun),
    LineBreak,
}

/// A run of text sharing the same formatting, as suggested by the Matrix specification.
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct TextRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub code: bool,
    pub color: Option<DePlaceColor>,
    pub background: Option<DePlaceColor>,
    /// `Some` if this run is inside a spoiler (`data-mx-spoiler`); the string is the reason for
    /// the spoiler, empty if none was given.
    pub spoiler: Option<String>,
    pub link: Option<MessageLink>,
}

/// Where a `<a>` tag in a message body points to.
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum MessageLink {
    User(OwnedUserId),
    Room(OwnedRoomId),
    RoomAlias(OwnedRoomAliasId),
    Event(OwnedRoomOrAliasId, OwnedEventId),

    Url(String),
}

/// The formatting inherited by the content of the HTML element currently being walked.
#[derive(Debug, Clone, Default)]
struct InlineStyle {
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    code: bool,
    color: Option<DePlaceColor>,
    background: Option<DePlaceColor>,
    spoiler: Option<String>,
    link: Option<MessageLink>,
}

impl FormattedBody {
    /// Parses the `formatted_body` of a `m.text`/`m.emote`/`m.notice` message into a
    /// [`FormattedBody`].
    pub fn parse_formatted_body(html: &str) -> FormattedBody {
        let fragment = Html::parse_fragment(html);

        let mut blocks = Vec::new();
        let mut pending = Vec::new();

        for node in fragment.tree.root().children() {
            walk_block(node, &InlineStyle::default(), &mut blocks, &mut pending);
        }
        flush_paragraph(&mut blocks, &mut pending);

        FormattedBody { blocks }
    }
}

fn flush_paragraph(blocks: &mut Vec<Block>, pending: &mut Vec<Inline>) {
    // Insignificant whitespace (e.g. the indentation newline between `<li>` and a wrapped
    // `<p>`) would otherwise flush as its own blank paragraph, showing up as a phantom empty
    // line before the real content.
    let is_blank = pending.iter().all(|inline| match inline {
        Inline::Text(run) => run.text.trim().is_empty(),
        Inline::LineBreak => true,
    });

    if !pending.is_empty() && !is_blank {
        blocks.push(Block::Paragraph(std::mem::take(pending)));
    } else {
        pending.clear();
    }
}

/// Parses the block-level children of a node (e.g. a `<li>` or `<blockquote>`) into a list of
/// [`Block`]s.
fn parse_blocks(node: NodeRef<'_, Node>, style: &InlineStyle) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut pending = Vec::new();

    for child in node.children() {
        walk_block(child, style, &mut blocks, &mut pending);
    }
    flush_paragraph(&mut blocks, &mut pending);

    blocks
}

/// Walks a node that can appear at block level, pushing completed blocks into `blocks` and
/// accumulating any loose inline content (not wrapped in a block tag) into `pending`.
fn walk_block(
    node: NodeRef<'_, Node>,
    style: &InlineStyle,
    blocks: &mut Vec<Block>,
    pending: &mut Vec<Inline>,
) {
    let elem = match node.value() {
        Node::Element(elem) => elem,
        // Text (or anything else) found outside of a block tag is still part of the surrounding
        // paragraph.
        _ => {
            walk_inline(node, style, pending);
            return;
        }
    };

    match elem.name() {
        "html" | "body" => {
            for child in node.children() {
                walk_block(child, style, blocks, pending);
            }
        }
        "p" | "div" => {
            flush_paragraph(blocks, pending);
            let mut content = Vec::new();
            for child in node.children() {
                walk_inline(child, style, &mut content);
            }
            if !content.is_empty() {
                blocks.push(Block::Paragraph(content));
            }
        }
        name @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
            flush_paragraph(blocks, pending);
            let level = name.as_bytes()[1] - b'0';
            let mut content = Vec::new();
            for child in node.children() {
                walk_inline(child, style, &mut content);
            }
            blocks.push(Block::Heading { level, content });
        }
        "blockquote" => {
            flush_paragraph(blocks, pending);
            blocks.push(Block::BlockQuote(parse_blocks(node, style)));
        }
        "ul" | "ol" => {
            flush_paragraph(blocks, pending);
            let ordered = elem.name() == "ol";
            let start = elem.attr("start").and_then(|start| start.parse().ok());
            let items = node
                .children()
                .filter(|child| matches!(child.value(), Node::Element(e) if e.name() == "li"))
                .map(|li| parse_blocks(li, style))
                .collect();
            blocks.push(Block::List {
                ordered,
                start,
                items,
            });
        }
        "pre" => {
            flush_paragraph(blocks, pending);
            let code_child = node
                .children()
                .find(|child| matches!(child.value(), Node::Element(e) if e.name() == "code"));
            let (language, source) = match code_child {
                Some(code_child) => {
                    let language = match code_child.value() {
                        Node::Element(e) => e.attr("class").and_then(|class| {
                            class
                                .split_whitespace()
                                .find_map(|c| c.strip_prefix("language-"))
                                .map(str::to_string)
                        }),
                        _ => None,
                    };
                    (language, code_child)
                }
                None => (None, node),
            };
            blocks.push(Block::CodeBlock {
                language,
                code: extract_text(source),
            });
        }
        "hr" => {
            flush_paragraph(blocks, pending);
            blocks.push(Block::ThematicBreak);
        }
        // Not a recognized block tag: treat it like inline content so its text isn't lost.
        _ => walk_inline(node, style, pending),
    }
}

/// Walks a node that can appear at inline level, pushing [`Inline`]s into `out`.
fn walk_inline(node: NodeRef<'_, Node>, style: &InlineStyle, out: &mut Vec<Inline>) {
    match node.value() {
        Node::Text(text) => {
            let content = text.text.replace("\u{a0}", " ");
            if !content.is_empty() {
                out.push(Inline::Text(TextRun {
                    text: content,
                    bold: style.bold,
                    italic: style.italic,
                    underline: style.underline,
                    strikethrough: style.strikethrough,
                    code: style.code,
                    color: style.color,
                    background: style.background,
                    spoiler: style.spoiler.clone(),
                    link: style.link.clone(),
                }));
            }
        }
        Node::Element(elem) => match elem.name() {
            "br" => out.push(Inline::LineBreak),
            "b" | "strong" => with_style(node, style, out, |s| s.bold = true),
            "i" | "em" => with_style(node, style, out, |s| s.italic = true),
            "u" => with_style(node, style, out, |s| s.underline = true),
            "s" | "del" => with_style(node, style, out, |s| s.strikethrough = true),
            "code" => with_style(node, style, out, |s| s.code = true),
            "span" => {
                let background = elem.attr("data-mx-bg-color").and_then(parse_color);
                let color = elem.attr("data-mx-color").and_then(parse_color);
                let spoiler = elem.attr("data-mx-spoiler").map(str::to_string);
                with_style(node, style, out, move |s| {
                    if background.is_some() {
                        s.background = background;
                    }
                    if color.is_some() {
                        s.color = color;
                    }
                    if spoiler.is_some() {
                        s.spoiler = spoiler;
                    }
                });
            }
            "a" => {
                let link = elem.attr("href").map(parse_link_target);
                with_style(node, style, out, move |s| s.link = link);
            }
            // Not a recognized inline tag: keep its text content.
            _ => {
                for child in node.children() {
                    walk_inline(child, style, out);
                }
            }
        },
        _ => {}
    }
}

/// Clones `style`, applies `apply` to the clone, then walks the children of `node` as inline
/// content with that updated style.
fn with_style(
    node: NodeRef<'_, Node>,
    style: &InlineStyle,
    out: &mut Vec<Inline>,
    apply: impl FnOnce(&mut InlineStyle),
) {
    let mut child_style = style.clone();
    apply(&mut child_style);
    for child in node.children() {
        walk_inline(child, &child_style, out);
    }
}

/// Recognizes `matrix:` and `https://matrix.to` URIs as mentions; anything else is a plain URL.
fn parse_link_target(href: &str) -> MessageLink {
    if let Ok(uri) = MatrixUri::parse(href) {
        return match uri.id().clone() {
            MatrixId::Event(room_id, event_id) => MessageLink::Event(room_id, event_id),
            MatrixId::User(user_id) => MessageLink::User(user_id),
            MatrixId::Room(room_id) => MessageLink::Room(room_id),
            MatrixId::RoomAlias(room_alias_id) => MessageLink::RoomAlias(room_alias_id),
            _ => MessageLink::Url(href.to_string()),
        };
    }
    if let Ok(uri) = MatrixToUri::parse(href) {
        return match uri.id().clone() {
            MatrixId::Event(room_id, event_id) => MessageLink::Event(room_id, event_id),
            MatrixId::User(user_id) => MessageLink::User(user_id),
            MatrixId::Room(room_id) => MessageLink::Room(room_id),
            MatrixId::RoomAlias(room_alias_id) => MessageLink::RoomAlias(room_alias_id),
            _ => MessageLink::Url(href.to_string()),
        };
    }
    MessageLink::Url(href.to_string())
}

fn parse_color(value: &str) -> Option<DePlaceColor> {
    value
        .parse::<csscolorparser::Color>()
        .ok()
        .map(DePlaceColor::from)
}

fn extract_text(node: NodeRef<'_, Node>) -> String {
    let mut text = String::new();
    for child in node.children() {
        match child.value() {
            Node::Text(t) => text.push_str(&t.text),
            _ => text.push_str(&extract_text(child)),
        }
    }
    text
}
