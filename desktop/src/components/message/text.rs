use std::sync::Arc;

use gpui::SharedString;
use matrix_sdk::ruma::{
    OwnedEventId, OwnedMxcUri, OwnedRoomOrAliasId, OwnedUserId, matrix_uri::MatrixId,
};
use ruma_html::{
    Html, NodeData, NodeRef,
    matrix::{AnchorData, AnchorUri, MatrixElement},
};

/// Parses a `formatted_body` HTML string (already sanitized by matrix-sdk-ui) into the
/// block tree used for rendering.
///
/// This walks the `html5ever` DOM exactly once and flattens it into `CachedBlock`s; the
/// DOM itself (`Rc<RefCell<_>>`-based, not `Send`, expensive to clone) is dropped at the
/// end of this function. Only the returned `Arc` should ever be stored - it is the single
/// point of sharing for the whole tree, so nothing nested inside `CachedBlock` needs its
/// own `Arc`.
pub fn convert_formatted_body(html: &str) -> Arc<[CachedBlock]> {
    let dom = Html::parse(html);

    let mut blocks = Vec::new();
    convert_blocks(dom.children(), &mut blocks);
    blocks.into()
}

fn matrix_element(node: &NodeRef) -> Option<MatrixElement> {
    node.as_element().map(|el| el.to_matrix().element)
}

/// Converts a run of sibling nodes into blocks.
///
/// `html5ever` does not require loose inline content (a bare mention, plain text, ...)
/// to be wrapped in `<p>` - it's routinely a direct sibling of real block elements. So
/// this doesn't dispatch each child independently: it accumulates consecutive non-block
/// children (text, `<a>`, `<b>`, ...) into one implicit paragraph via `convert_inline`,
/// and only flushes that paragraph when a real block element is hit. A naive per-child
/// dispatch would otherwise unwrap a top-level `<a>` down to its bare text and lose the
/// link/pill data entirely.
fn convert_blocks(children: impl Iterator<Item = NodeRef>, out: &mut Vec<CachedBlock>) {
    let mut pending = RunBuilder::default();

    for child in children {
        let Some(element) = matrix_element(&child) else {
            if let NodeData::Text(text) = child.data() {
                pending.push(&text.borrow(), CachedRunStyle::default());
            }
            continue;
        };

        match element {
            MatrixElement::P => {
                flush_pending(&mut pending, out);
                out.push(CachedBlock::Paragraph(convert_rich_text(&child)));
            }
            MatrixElement::H(heading) => {
                flush_pending(&mut pending, out);
                out.push(CachedBlock::Heading {
                    level: heading.level.value(),
                    text: convert_rich_text(&child),
                });
            }
            MatrixElement::Ul => {
                flush_pending(&mut pending, out);
                out.push(CachedBlock::List {
                    ordered: false,
                    start: None,
                    items: convert_list_items(&child),
                });
            }
            MatrixElement::Ol(data) => {
                flush_pending(&mut pending, out);
                out.push(CachedBlock::List {
                    ordered: true,
                    start: data.start,
                    items: convert_list_items(&child),
                });
            }
            MatrixElement::Blockquote => {
                flush_pending(&mut pending, out);
                let mut quoted = Vec::new();
                convert_blocks(child.children(), &mut quoted);
                out.push(CachedBlock::Quote(quoted));
            }
            MatrixElement::Pre => {
                flush_pending(&mut pending, out);
                out.push(convert_code_block(&child));
            }
            MatrixElement::Hr => {
                flush_pending(&mut pending, out);
                out.push(CachedBlock::Rule);
            }
            MatrixElement::Img(img) => {
                if let Some(src) = img.src {
                    flush_pending(&mut pending, out);
                    out.push(CachedBlock::Image {
                        src,
                        alt: img.alt.map(|a| a.as_ref().into()),
                        width: img.width.and_then(|w| u32::try_from(w).ok()),
                        height: img.height.and_then(|h| u32::try_from(h).ok()),
                    });
                }
            }
            // Stale rich-reply fallback: drop the element and its content entirely
            // instead of recursing into it. matrix-sdk-ui already strips this in the
            // normal case.
            MatrixElement::MatrixReply => {}
            // Transparent containers (the synthetic `<html>` root, a bare `<div>`,
            // anything the sanitizer left alone but that isn't itself renderable):
            // their children can still mix blocks with loose inline content, so run
            // them through the same collector rather than assuming one or the other.
            MatrixElement::Div(_) | MatrixElement::Other(_) => {
                flush_pending(&mut pending, out);
                convert_blocks(child.children(), out);
            }
            // Genuine inline content sitting at block position with no `<p>` wrapper
            // (a mention, bold text, ...): fold it into the paragraph being built.
            _ => convert_inline(&child, CachedRunStyle::default(), &mut pending),
        }
    }

    flush_pending(&mut pending, out);
}

fn flush_pending(pending: &mut RunBuilder, out: &mut Vec<CachedBlock>) {
    if pending.text.trim().is_empty() {
        pending.text.clear();
        pending.runs.clear();
        return;
    }
    out.push(CachedBlock::Paragraph(std::mem::take(pending).finish()));
}

fn convert_list_items(node: &NodeRef) -> Vec<Vec<CachedBlock>> {
    node.children()
        .filter(|child| matches!(matrix_element(child), Some(MatrixElement::Li)))
        .map(|li| {
            let mut blocks = Vec::new();
            convert_blocks(li.children(), &mut blocks);
            blocks
        })
        .collect()
}

fn convert_code_block(node: &NodeRef) -> CachedBlock {
    // Per the Matrix spec, `<pre>` wraps a single `<code>` that may carry the language
    // as a `language-*` class. Fall back to the `<pre>`'s own text if that shape isn't there.
    let code_node = node
        .children()
        .find(|child| matches!(matrix_element(child), Some(MatrixElement::Code(_))));

    let language = code_node
        .as_ref()
        .and_then(|code| match matrix_element(code) {
            Some(MatrixElement::Code(data)) => data.language.map(|l| l.as_ref().into()),
            _ => None,
        });

    let text_node = code_node.as_ref().unwrap_or(node);
    let code = collect_text(text_node);

    CachedBlock::CodeBlock {
        code: code.into(),
        language,
    }
}

fn collect_text(node: &NodeRef) -> String {
    let mut text = String::new();
    collect_text_into(node, &mut text);
    text
}

fn collect_text_into(node: &NodeRef, out: &mut String) {
    match node.data() {
        NodeData::Text(text) => out.push_str(&text.borrow()),
        _ => {
            for child in node.children() {
                collect_text_into(&child, out);
            }
        }
    }
}

fn convert_rich_text(node: &NodeRef) -> CachedRichText {
    let mut builder = RunBuilder::default();
    for child in node.children() {
        convert_inline(&child, CachedRunStyle::default(), &mut builder);
    }
    builder.finish()
}

fn convert_inline(node: &NodeRef, style: CachedRunStyle, out: &mut RunBuilder) {
    match node.data() {
        NodeData::Text(text) => out.push(&text.borrow(), style),
        NodeData::Element(_) => {
            let Some(element) = matrix_element(node) else {
                return;
            };

            match element {
                MatrixElement::B | MatrixElement::Strong => recurse_inline(
                    node,
                    CachedRunStyle {
                        bold: true,
                        ..style
                    },
                    out,
                ),
                MatrixElement::I | MatrixElement::Em => recurse_inline(
                    node,
                    CachedRunStyle {
                        italic: true,
                        ..style
                    },
                    out,
                ),
                MatrixElement::U => recurse_inline(
                    node,
                    CachedRunStyle {
                        underline: true,
                        ..style
                    },
                    out,
                ),
                MatrixElement::S | MatrixElement::Del => recurse_inline(
                    node,
                    CachedRunStyle {
                        strikethrough: true,
                        ..style
                    },
                    out,
                ),
                MatrixElement::Code(_) => recurse_inline(
                    node,
                    CachedRunStyle {
                        code: true,
                        ..style
                    },
                    out,
                ),
                MatrixElement::Span(span) if span.spoiler.is_some() => recurse_inline(
                    node,
                    CachedRunStyle {
                        spoiler: span.spoiler.map(|s| s.as_ref().into()),
                        ..style
                    },
                    out,
                ),
                MatrixElement::A(anchor) => recurse_inline(
                    node,
                    CachedRunStyle {
                        link: convert_anchor(&anchor),
                        ..style
                    },
                    out,
                ),
                MatrixElement::Br => out.push("\n", style),
                _ => recurse_inline(node, style, out),
            }
        }
        _ => {}
    }
}

fn recurse_inline(node: &NodeRef, style: CachedRunStyle, out: &mut RunBuilder) {
    for child in node.children() {
        convert_inline(&child, style.clone(), out);
    }
}

fn convert_anchor(anchor: &AnchorData) -> Option<CachedLink> {
    match &anchor.href {
        Some(AnchorUri::Other(href)) => Some(CachedLink::Url(href.as_ref().into())),
        Some(uri @ (AnchorUri::Matrix(_) | AnchorUri::MatrixTo(_))) => {
            convert_pill(uri).map(CachedLink::Pill)
        }
        Some(_) | None => None,
    }
}

fn convert_pill(uri: &AnchorUri) -> Option<CachedPill> {
    let id = match uri {
        AnchorUri::Matrix(uri) => uri.id(),
        AnchorUri::MatrixTo(uri) => uri.id(),
        AnchorUri::Other(_) => return None,
        _ => return None,
    };

    match id.clone() {
        MatrixId::User(user) => Some(CachedPill::User(user)),
        MatrixId::Room(room) => Some(CachedPill::Room(room.into())),
        MatrixId::RoomAlias(alias) => Some(CachedPill::Room(alias.into())),
        MatrixId::Event(room, event) => Some(CachedPill::Event { room, event }),
        _ => None,
    }
}

#[derive(Default)]
struct RunBuilder {
    text: String,
    runs: Vec<CachedRun>,
}

impl RunBuilder {
    fn push(&mut self, text: &str, style: CachedRunStyle) {
        if text.is_empty() {
            return;
        }

        self.text.push_str(text);
        match self.runs.last_mut() {
            Some(last) if last.style == style => last.len += text.len(),
            _ => self.runs.push(CachedRun {
                len: text.len(),
                style,
            }),
        }
    }

    fn finish(self) -> CachedRichText {
        CachedRichText {
            text: self.text.into(),
            runs: self.runs.into(),
        }
    }
}

// This tree only ever lives behind the single `Arc<[CachedBlock]>` returned by
// `convert_formatted_body` (stored once on the owning message). Nothing nested in here
// needs its own `Arc`: cloning the message clones that one outer `Arc` (a refcount bump),
// never an individual block/run/id out of the middle of the tree.
#[derive(Debug)]
pub(crate) enum CachedBlock {
    Paragraph(CachedRichText),
    Heading {
        level: u8,
        text: CachedRichText,
    },
    List {
        ordered: bool,
        start: Option<i64>,
        items: Vec<Vec<CachedBlock>>,
    },
    CodeBlock {
        code: SharedString,
        language: Option<SharedString>,
    },
    Quote(Vec<CachedBlock>),
    Rule,
    Image {
        src: OwnedMxcUri,
        alt: Option<SharedString>,
        width: Option<u32>,
        height: Option<u32>,
    },
    // table / details / anything else -> "not supported yet" text.
}

impl CachedBlock {
    pub fn new_plain(text: &str) -> Self {
        Self::Paragraph(CachedRichText {
            text: text.into(),
            runs: Box::new([]),
        })
    }
}

#[derive(Debug)]
pub(crate) struct CachedRichText {
    pub(crate) text: SharedString,
    pub(crate) runs: Box<[CachedRun]>,
}

#[derive(Debug)]
pub(crate) struct CachedRun {
    pub(crate) len: usize,
    pub(crate) style: CachedRunStyle,
}

// A *struct of flags*, not an enum — tags nest (bold link, spoiler containing italic, etc.),
// so a run needs to represent the combined effect of everything wrapping it.
#[derive(Default, Clone, PartialEq, Debug)]
pub(crate) struct CachedRunStyle {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strikethrough: bool,
    pub(crate) code: bool,
    pub(crate) spoiler: Option<SharedString>, // reason, if any
    pub(crate) link: Option<CachedLink>,
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum CachedLink {
    Url(SharedString),
    Pill(CachedPill),
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum CachedPill {
    User(OwnedUserId),
    Room(OwnedRoomOrAliasId),
    Event {
        room: OwnedRoomOrAliasId,
        event: OwnedEventId,
    },
}

// Rendering (`CachedBlock::render` / `CachedRichText::render`) lives in `render.rs`,
// next to the rest of this module's `render()` methods.
