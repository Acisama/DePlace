//! Parsing of the `formatted_body` (HTML, as suggested by the Matrix specification) of a message
//! into a structure that is convenient to render.

use std::hash::Hash;

use ruma::{
    OwnedEventId, OwnedRoomAliasId, OwnedRoomId, OwnedRoomOrAliasId, OwnedUserId,
    matrix_uri::MatrixId,
};
use ruma_html::{
    Html, NodeData, NodeRef,
    matrix::{AnchorUri, CodeData, MatrixElement, OrderedListData},
};

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

#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Mention {
    User {
        user_id: OwnedUserId,
        display_name: Option<String>,
    },
    Event {
        room_or_alias_id: OwnedRoomOrAliasId,
        event_id: OwnedEventId,
    },
    Room(OwnedRoomId),
    RoomAlias(OwnedRoomAliasId),
}

/// An inline element within a [`Block`].
#[derive(Debug, Clone, PartialEq, Hash)]
pub enum Inline {
    Text(TextRun),
    LineBreak,
    Mention(Mention),
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
    pub link: Option<String>,
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
    link: Option<String>,
}

impl FormattedBody {
    /// Parses the `formatted_body` of a `m.text`/`m.emote`/`m.notice` message into a
    /// [`FormattedBody`].
    pub fn parse_formatted_body(html: &str) -> FormattedBody {
        let fragment = Html::parse(html);

        let mut blocks = Vec::new();
        let mut pending = Vec::new();

        for node in fragment.children() {
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
        Inline::Mention(_) => false,
    });

    if !pending.is_empty() && !is_blank {
        blocks.push(Block::Paragraph(std::mem::take(pending)));
    } else {
        pending.clear();
    }
}

/// Parses the block-level children of a node (e.g. a `<li>` or `<blockquote>`) into a list of
/// [`Block`]s.
fn parse_blocks(node: NodeRef, style: &InlineStyle) -> Vec<Block> {
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
    node: NodeRef,
    style: &InlineStyle,
    blocks: &mut Vec<Block>,
    pending: &mut Vec<Inline>,
) {
    let elem = match node.as_element() {
        Some(elem) => elem,
        // Text (or anything else) found outside of a block tag is still part of the surrounding
        // paragraph.
        None => {
            walk_inline(node, style, pending);
            return;
        }
    };

    match elem.to_matrix().element {
        MatrixElement::P | MatrixElement::Div(_) => {
            flush_paragraph(blocks, pending);
            let mut content = Vec::new();
            for child in node.children() {
                walk_inline(child, style, &mut content);
            }
            if !content.is_empty() {
                blocks.push(Block::Paragraph(content));
            }
        }
        MatrixElement::H(heading) => {
            flush_paragraph(blocks, pending);
            let mut content = Vec::new();
            for child in node.children() {
                walk_inline(child, style, &mut content);
            }
            blocks.push(Block::Heading {
                level: heading.level.value(),
                content,
            });
        }
        MatrixElement::Blockquote => {
            flush_paragraph(blocks, pending);
            blocks.push(Block::BlockQuote(parse_blocks(node, style)));
        }
        MatrixElement::Ul => finish_list(node, style, blocks, pending, false, None),
        MatrixElement::Ol(OrderedListData { start, .. }) => {
            finish_list(node, style, blocks, pending, true, start)
        }
        MatrixElement::Pre => {
            flush_paragraph(blocks, pending);
            let code_child = node.children().find(|child| {
                matches!(
                    child.as_element().map(|e| e.to_matrix().element),
                    Some(MatrixElement::Code(_))
                )
            });
            let (language, source) = match &code_child {
                Some(child) => {
                    let language = match child.as_element().map(|e| e.to_matrix().element) {
                        Some(MatrixElement::Code(CodeData { language, .. })) => {
                            language.map(|l| l.to_string())
                        }
                        _ => None,
                    };
                    (language, child.clone())
                }
                None => (None, node.clone()),
            };
            blocks.push(Block::CodeBlock {
                language,
                code: extract_text(&source),
            });
        }
        MatrixElement::Hr => {
            flush_paragraph(blocks, pending);
            blocks.push(Block::ThematicBreak);
        }
        // Not a recognized block tag: treat it like inline content so its text isn't lost.
        _ => walk_inline(node, style, pending),
    }
}

/// Finishes parsing a `<ul>`/`<ol>` element into a [`Block::List`].
fn finish_list(
    node: NodeRef,
    style: &InlineStyle,
    blocks: &mut Vec<Block>,
    pending: &mut Vec<Inline>,
    ordered: bool,
    start: Option<i64>,
) {
    flush_paragraph(blocks, pending);
    let items = node
        .children()
        .filter(|child| {
            matches!(
                child.as_element().map(|e| e.to_matrix().element),
                Some(MatrixElement::Li)
            )
        })
        .map(|li| parse_blocks(li, style))
        .collect();
    blocks.push(Block::List {
        ordered,
        start,
        items,
    });
}

/// Walks a node that can appear at inline level, pushing [`Inline`]s into `out`.
fn walk_inline(node: NodeRef, style: &InlineStyle, out: &mut Vec<Inline>) {
    let elem = match node.as_element() {
        Some(elem) => elem,
        None => {
            if let NodeData::Text(text) = node.data() {
                let content = text.borrow().replace('\u{a0}', " ");
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
            return;
        }
    };

    match elem.to_matrix().element {
        MatrixElement::Br => out.push(Inline::LineBreak),
        MatrixElement::B | MatrixElement::Strong => with_style(node, style, out, |s| s.bold = true),
        MatrixElement::I | MatrixElement::Em => with_style(node, style, out, |s| s.italic = true),
        MatrixElement::U => with_style(node, style, out, |s| s.underline = true),
        MatrixElement::S | MatrixElement::Del => {
            with_style(node, style, out, |s| s.strikethrough = true)
        }
        MatrixElement::Code(_) => with_style(node, style, out, |s| s.code = true),
        MatrixElement::Span(span) => {
            let background = span.bg_color.and_then(|c| parse_color(&c));
            let color = span.color.and_then(|c| parse_color(&c));
            let spoiler = span.spoiler.map(|s| s.to_string());
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
        MatrixElement::A(anchor) => match anchor.href.and_then(mention_or_link) {
            Some(LinkTarget::Mention(Mention::User { user_id, .. })) => {
                let name = extract_text(&node);
                let name = (!name.trim().is_empty()).then_some(name);
                out.push(Inline::Mention(Mention::User {
                    user_id,
                    display_name: name,
                }));
            }
            Some(LinkTarget::Mention(mention)) => out.push(Inline::Mention(mention)),
            Some(LinkTarget::Url(url)) => {
                with_style(node, style, out, move |s| s.link = Some(url));
            }
            None => {
                for child in node.children() {
                    walk_inline(child, style, out);
                }
            }
        },
        // Not a recognized inline tag: keep its text content.
        _ => {
            for child in node.children() {
                walk_inline(child, style, out);
            }
        }
    }
}

/// Clones `style`, applies `apply` to the clone, then walks the children of `node` as inline
/// content with that updated style.
fn with_style(
    node: NodeRef,
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

enum LinkTarget {
    Mention(Mention),
    Url(String),
}

/// Recognizes `matrix:` and `https://matrix.to` URIs pointing to a user as mentions; anything
/// else (including mentions of rooms, aliases or events) is kept as a plain link.
fn mention_or_link(href: AnchorUri) -> Option<LinkTarget> {
    match href {
        AnchorUri::Matrix(uri) => mention_from_id(uri.id().clone()).map(LinkTarget::Mention),
        AnchorUri::MatrixTo(uri) => mention_from_id(uri.id().clone()).map(LinkTarget::Mention),
        AnchorUri::Other(url) => Some(LinkTarget::Url(url.to_string())),
        _ => None,
    }
}

fn mention_from_id(id: MatrixId) -> Option<Mention> {
    match id {
        MatrixId::User(user_id) => Some(Mention::User {
            user_id,
            display_name: None,
        }),
        MatrixId::Event(room_id, event_id) => Some(Mention::Event {
            room_or_alias_id: room_id,
            event_id,
        }),
        MatrixId::Room(room_id) => Some(Mention::Room(room_id)),
        MatrixId::RoomAlias(room_alias_id) => Some(Mention::RoomAlias(room_alias_id)),
        _ => None,
    }
}

fn parse_color(value: &str) -> Option<DePlaceColor> {
    value
        .parse::<csscolorparser::Color>()
        .ok()
        .map(DePlaceColor::from)
}

fn extract_text(node: &NodeRef) -> String {
    let mut text = String::new();
    for child in node.children() {
        match child.data() {
            NodeData::Text(t) => text.push_str(&t.borrow()),
            _ => text.push_str(&extract_text(&child)),
        }
    }
    text
}
