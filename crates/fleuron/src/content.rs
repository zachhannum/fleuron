//! The content tree: semantic input.
//!
//! The markdown frontend produces this; the element vocabulary is
//! bounded by what a book needs — book/section, heading, paragraph,
//! blockquote, thematic break, emphasis/strong/code, image, link.
//!
//! This module is the **input contract**: everything downstream (style,
//! box construction, layout) consumes these types, and nothing widens
//! the vocabulary without a fixture and a test. It is a Rust type, and
//! that is the seam a frontend of its own builds against: a docx or CMS
//! reader constructs a `Book` directly, with the compiler checking the
//! shape.
//!
//! # Reading a tree back
//!
//! The tree serializes, internally tagged (`{"type": "paragraph", …}`)
//! so the shape maps one-to-one onto mdast. It is mostly an output: it
//! is how to see what a frontend made of a manuscript. It reads back
//! too, which is the door a host with a structured source of its own
//! comes in through, so what the engine writes is what a host may hand
//! it again.
//!
//! # Node identity
//!
//! `NodeId` is engine-assigned, never frontend-supplied: input can't
//! collide ids or forge diagnostic origins. Every node's `id` field is
//! `#[serde(skip)]`, so a serialized tree has none. The ids in a tree
//! built by hand are `NodeId::UNASSIGNED` until `Book::assign_node_ids`
//! assigns dense ids from 1 in document order (pre-order: a node before
//! its children, sections in reading order).
//!
//! # Source positions
//!
//! Every node has an optional 1-based line/column into the markdown
//! source the frontend read it from; the section's `source` names the
//! file. `origin` formats the pair for diagnostics
//! (`chapter-01.md:12:3`). A missing position never fails a run.
//!
//! Beside it every node has an optional `span`: the bytes of that
//! source the node was read from, markup included. [`Book::node_at`]
//! turns a byte of a source into the node written there and
//! [`Book::source_of`] turns a node back into the bytes it was read
//! from, which is how a host holding the manuscript maps a cursor
//! onto a page and a run under the pointer back onto the file it was
//! written in. A tree built rather than parsed has neither, and both
//! questions answer with nothing.

use std::collections::BTreeMap;
use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Identity of one node in the content tree, for diagnostics and
/// incremental relayout.
///
/// Assigned in document order, starting at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct NodeId(u32);

impl NodeId {
    /// What every node's id is before assignment.
    pub const UNASSIGNED: NodeId = NodeId(0);

    /// The raw id. Monotonic in document order within one book.
    pub fn get(self) -> u32 {
        self.0
    }

    /// The id this one becomes when the section around it is
    /// renumbered by `step`. A section's nodes are dense and in
    /// document order from its own id, so one step moves all of
    /// them. Unassigned stays unassigned.
    pub(crate) fn shifted(self, step: i64) -> NodeId {
        if self == NodeId::UNASSIGNED {
            return self;
        }
        NodeId((self.0 as i64 + step).max(0) as u32)
    }
}

/// A stretch of one node's text: the node it was written in, and
/// the bytes of that node's own text the stretch covers.
///
/// The range indexes the node's text as the frontend read it, before
/// `text-transform` or a synthesized small capital changed what was
/// shaped.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceRange {
    /// The node the text was written in.
    pub node: NodeId,
    /// Byte range in that node's own text.
    pub range: Range<u32>,
}

/// The bytes of one source a node was read from: its extent in the
/// file, markup included.
///
/// A source and the text of the nodes read from it are different
/// bytes, because markup is not text. The span is the node's extent
/// rather than a character-by-character map, so a byte of the source
/// lands on the node written there and not on a letter of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    /// First byte of the source the node was read from.
    pub start: u32,
    /// One past the last.
    pub end: u32,
}

impl SourceSpan {
    /// Whether a byte of the source falls in the span. The end is
    /// past it, so the spans of two nodes written one after the other
    /// answer for their own bytes and no others.
    pub fn covers(self, byte: u32) -> bool {
        (self.start..self.end).contains(&byte)
    }

    /// How many bytes of the source it covers.
    fn width(self) -> u32 {
        self.end.saturating_sub(self.start)
    }
}

/// A 1-based position in the frontend's source document.
///
/// Line and column are as the markdown parser reported them. This is
/// diagnostic data, never layout input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourcePos {
    /// 1-based line in the source markdown.
    pub line: u32,
    /// 1-based column in the source markdown.
    pub column: u32,
}

/// Formats a file name plus a position for diagnostics:
/// `chapter-01.md:12:3`. Missing parts degrade: bare file name, bare
/// position, empty string.
pub fn origin(source: Option<&str>, position: Option<SourcePos>) -> String {
    match (source, position) {
        (Some(file), Some(pos)) => format!("{file}:{}:{}", pos.line, pos.column),
        (Some(file), None) => file.to_string(),
        (None, Some(pos)) => format!("{}:{}", pos.line, pos.column),
        (None, None) => String::new(),
    }
}

/// Book metadata: everything about the work that isn't content.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Metadata {
    /// Title, for the half-title and running heads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Author, for the title page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Frontend-defined extensions (language, ISBN, subtitle…) keyed
    /// by name. Opaque to the engine; style reads them, layout
    /// doesn't.
    ///
    /// Left out of the JSON when empty, and so it has to be
    /// optional coming back in: what the engine writes is what a
    /// host hands it again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

impl Metadata {
    /// The BCP 47 tag under `extra["language"]`. The PDF records it
    /// and hyphenation takes its patterns from it. A blank tag counts
    /// as none declared.
    pub fn language(&self) -> Option<&str> {
        self.extra
            .get("language")
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
    }
}

/// The root of the content tree: one book.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Book {
    /// The work's title, author and frontend extensions.
    pub metadata: Metadata,
    /// The chapters/files, in reading order.
    pub sections: Vec<Section>,
}

/// A chapter or file: the unit of markdown input and of source
/// attribution for diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Engine-assigned identity, for diagnostics; never serialized.
    #[serde(skip)]
    pub id: NodeId,
    /// File the frontend read (e.g. `chapter-01.md`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Section title supplied outside the body (frontmatter
    /// `title:`); implies heading level 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The section's blocks, in reading order.
    pub blocks: Vec<Block>,
    /// Where the frontend read this from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<SourcePos>,
    /// The bytes of that source it was read from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
}

/// A block-level element: the unit of fragmentation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// `#` through `######`; levels outside 1–6 are rejected at parse.
    Heading {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// `#` count, 1-6.
        level: HeadingLevel,
        /// The heading's text, in reading order.
        inlines: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// A run of prose: the unit line layout breaks.
    Paragraph {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// The paragraph's text, in reading order.
        inlines: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// A quotation set off by `>`; contents are blocks, not inlines —
    /// blockquotes nest.
    Blockquote {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// The quoted blocks, in reading order.
        blocks: Vec<Block>,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// `---`: a scene break, rendered as space or an ornament (❦).
    ThematicBreak {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// A block-level image.
    Image {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// Where the image lives; the host resolves it, not the engine.
        url: String,
        /// Alt text: not laid out, but part of the accessibility
        /// contract.
        alt: String,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
}

/// A heading level: 1 to 6, the range markdown defines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum HeadingLevel {
    /// `#`
    H1,
    /// `##`
    H2,
    /// `###`
    H3,
    /// `####`
    H4,
    /// `#####`
    H5,
    /// `######`
    H6,
}

impl From<HeadingLevel> for u8 {
    fn from(level: HeadingLevel) -> u8 {
        match level {
            HeadingLevel::H1 => 1,
            HeadingLevel::H2 => 2,
            HeadingLevel::H3 => 3,
            HeadingLevel::H4 => 4,
            HeadingLevel::H5 => 5,
            HeadingLevel::H6 => 6,
        }
    }
}

impl TryFrom<u8> for HeadingLevel {
    type Error = InvalidHeadingLevel;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(HeadingLevel::H1),
            2 => Ok(HeadingLevel::H2),
            3 => Ok(HeadingLevel::H3),
            4 => Ok(HeadingLevel::H4),
            5 => Ok(HeadingLevel::H5),
            6 => Ok(HeadingLevel::H6),
            _ => Err(InvalidHeadingLevel(value)),
        }
    }
}

/// A heading level outside 1–6, with the offending value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("heading level must be 1-6, got {0}")]
pub struct InvalidHeadingLevel(pub u8);

/// An inline element: participates in line layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    /// A run of text. The frontend has already decoded entities; the
    /// engine sees plain Unicode.
    Text {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// The characters themselves, entities already decoded.
        value: String,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// `*emphasis*`: italic, in the default sheet.
    Emphasis {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// The emphasised inlines.
        children: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// `**strong**`: bold, in the default sheet.
    Strong {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// The strengthened inlines.
        children: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// `` `code` ``: monospace, and never hyphenated.
    Code {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// The literal code text; no markup inside.
        value: String,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
    /// A hyperlink. The text lays out; the url is for painters that can
    /// express one.
    Link {
        /// Engine-assigned identity, for diagnostics; never serialized.
        #[serde(skip)]
        id: NodeId,
        /// The link target.
        url: String,
        /// The linked inlines.
        children: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
        /// The bytes of that source it was read from.
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<SourceSpan>,
    },
}

/// The text of an inline tree, markup discarded: every inline run
/// together, as `content()` reads an element and as a frontend reads
/// alt text.
pub fn text(inlines: &[Inline]) -> String {
    let mut out = String::new();
    push_text(inlines, &mut out);
    out
}

fn push_text(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text { value, .. } | Inline::Code { value, .. } => out.push_str(value),
            Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Link { children, .. } => push_text(children, out),
        }
    }
}

impl Book {
    /// Assign ids to every node, in document order (pre-order: a node
    /// before its children, sections in reading order), starting at 1.
    /// Runs once, after deserialization; running it again renumbers.
    pub fn assign_node_ids(&mut self) {
        let mut next = 1u32;
        for section in &mut self.sections {
            section.id = next_id(&mut next);
            for block in &mut section.blocks {
                assign_block(block, &mut next);
            }
        }
    }

    /// The node one byte of one source was read into: the innermost,
    /// so a byte of prose answers with the run it was typed into and
    /// a byte of markup answers with the construct it opens.
    ///
    /// Only sections read from that source are looked at, so one
    /// file's cursor is answered by one file's nodes. Nothing for a
    /// byte no node was read from — a blank line between chapters,
    /// frontmatter — or for a tree built rather than parsed.
    pub fn node_at(&self, source: &str, byte: u32) -> Option<NodeId> {
        self.sections
            .iter()
            .filter(|section| section.source.as_deref() == Some(source))
            .find_map(|section| {
                let span = section.span.filter(|span| span.covers(byte))?;
                Some(narrowest(
                    (section.id, span),
                    node_in_blocks(&section.blocks, byte),
                ))
            })
            .map(|(node, _)| node)
    }

    /// The source a node was read from, and the bytes of it the node
    /// covers.
    ///
    /// Nothing for a node the engine synthesized, or one from a tree
    /// built rather than parsed: neither was read from anything.
    pub fn source_of(&self, node: NodeId) -> Option<(&str, SourceSpan)> {
        if node == NodeId::UNASSIGNED {
            return None;
        }
        // A section's nodes are dense and in document order from the
        // section's own id, so the section holding a node is the last
        // one numbered at or before it.
        let at = self
            .sections
            .partition_point(|section| section.id.get() <= node.get())
            .checked_sub(1)?;
        let section = &self.sections[at];
        let source = section.source.as_deref()?;
        let span = if section.id == node {
            section.span?
        } else {
            span_in_blocks(&section.blocks, node)?
        };
        Some((source, span))
    }
}

/// The narrower of a node and whichever of its descendants was read
/// from the same byte.
fn narrowest(
    node: (NodeId, SourceSpan),
    inner: Option<(NodeId, SourceSpan)>,
) -> (NodeId, SourceSpan) {
    match inner {
        Some(inner) if inner.1.width() <= node.1.width() => inner,
        _ => node,
    }
}

/// The innermost block or inline of these blocks a byte was read
/// into. Blocks are in source order, so a block starting past the
/// byte ends the search; an image written among prose is the one that
/// starts inside the paragraph it was moved out of, which is why the
/// narrowest span wins rather than the first.
fn node_in_blocks(blocks: &[Block], byte: u32) -> Option<(NodeId, SourceSpan)> {
    let mut found: Option<(NodeId, SourceSpan)> = None;
    for block in blocks {
        let Some(span) = block_span(block) else {
            continue;
        };
        if span.start > byte {
            break;
        }
        if !span.covers(byte) {
            continue;
        }
        let inner = match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                node_in_inlines(inlines, byte)
            }
            Block::Blockquote { blocks, .. } => node_in_blocks(blocks, byte),
            Block::ThematicBreak { .. } | Block::Image { .. } => None,
        };
        let hit = narrowest((block_id(block), span), inner);
        if found.is_none_or(|found| hit.1.width() < found.1.width()) {
            found = Some(hit);
        }
    }
    found
}

/// The same, over the inlines of one block.
fn node_in_inlines(inlines: &[Inline], byte: u32) -> Option<(NodeId, SourceSpan)> {
    for inline in inlines {
        let Some(span) = inline_span(inline) else {
            continue;
        };
        if span.start > byte {
            break;
        }
        if !span.covers(byte) {
            continue;
        }
        let inner = match inline {
            Inline::Text { .. } | Inline::Code { .. } => None,
            Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Link { children, .. } => node_in_inlines(children, byte),
        };
        return Some(narrowest((inline_id(inline), span), inner));
    }
    None
}

/// The span of one node of these blocks, by id.
fn span_in_blocks(blocks: &[Block], node: NodeId) -> Option<SourceSpan> {
    for block in blocks {
        if block_id(block) == node {
            return block_span(block);
        }
        let found = match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                span_in_inlines(inlines, node)
            }
            Block::Blockquote { blocks, .. } => span_in_blocks(blocks, node),
            Block::ThematicBreak { .. } | Block::Image { .. } => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The same, over the inlines of one block.
fn span_in_inlines(inlines: &[Inline], node: NodeId) -> Option<SourceSpan> {
    for inline in inlines {
        if inline_id(inline) == node {
            return inline_span(inline);
        }
        let found = match inline {
            Inline::Text { .. } | Inline::Code { .. } => None,
            Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Link { children, .. } => span_in_inlines(children, node),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// One block's identity.
pub fn block_id(block: &Block) -> NodeId {
    match block {
        Block::Heading { id, .. }
        | Block::Paragraph { id, .. }
        | Block::Blockquote { id, .. }
        | Block::ThematicBreak { id, .. }
        | Block::Image { id, .. } => *id,
    }
}

/// The bytes of its source one block was read from.
pub fn block_span(block: &Block) -> Option<SourceSpan> {
    match block {
        Block::Heading { span, .. }
        | Block::Paragraph { span, .. }
        | Block::Blockquote { span, .. }
        | Block::ThematicBreak { span, .. }
        | Block::Image { span, .. } => *span,
    }
}

/// One inline's identity.
pub fn inline_id(inline: &Inline) -> NodeId {
    match inline {
        Inline::Text { id, .. }
        | Inline::Code { id, .. }
        | Inline::Emphasis { id, .. }
        | Inline::Strong { id, .. }
        | Inline::Link { id, .. } => *id,
    }
}

/// The bytes of its source one inline was read from.
pub fn inline_span(inline: &Inline) -> Option<SourceSpan> {
    match inline {
        Inline::Text { span, .. }
        | Inline::Code { span, .. }
        | Inline::Emphasis { span, .. }
        | Inline::Strong { span, .. }
        | Inline::Link { span, .. } => *span,
    }
}

fn next_id(next: &mut u32) -> NodeId {
    let id = NodeId(*next);
    *next += 1;
    id
}

fn assign_block(block: &mut Block, next: &mut u32) {
    match block {
        Block::Heading { id, inlines, .. } | Block::Paragraph { id, inlines, .. } => {
            *id = next_id(next);
            for inline in inlines {
                assign_inline(inline, next);
            }
        }
        Block::Blockquote { id, blocks, .. } => {
            *id = next_id(next);
            for nested in blocks {
                assign_block(nested, next);
            }
        }
        Block::ThematicBreak { id, .. } | Block::Image { id, .. } => {
            *id = next_id(next);
        }
    }
}

fn assign_inline(inline: &mut Inline, next: &mut u32) {
    match inline {
        Inline::Text { id, .. } | Inline::Code { id, .. } => {
            *id = next_id(next);
        }
        Inline::Emphasis { id, children, .. }
        | Inline::Strong { id, children, .. }
        | Inline::Link { id, children, .. } => {
            *id = next_id(next);
            for child in children {
                assign_inline(child, next);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The markdown the sample tree was read from, so that its spans
    /// are the bytes of something rather than numbers made up.
    const SOURCE: &str = "\
# Chapter One

It was the kind of morning that made you suspicious — too *clean*, too quiet.

> \"Nobody's early here.\"

---

![The drawer of knives](images/drawer.png)
";

    /// The bytes of the sample source one stretch of it covers.
    fn span(of: &str) -> Option<SourceSpan> {
        let start = SOURCE.find(of).expect("the sample source has it");
        Some(SourceSpan {
            start: start as u32,
            end: (start + of.len()) as u32,
        })
    }

    /// Test-local shorthand: an unassigned text run, spanning the
    /// bytes of the source it reads as.
    fn text(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            position: None,
            span: span(value),
        }
    }

    /// What the engine writes, a host may hand back. The fields
    /// left out of the JSON when they are empty are the ones this
    /// catches: a book with no extras serializes without the map it
    /// then has to be readable without.
    #[test]
    fn a_tree_the_engine_wrote_reads_back() {
        for book in [sample_book(), Book::default()] {
            let json = serde_json::to_string(&book).unwrap();
            let read: Book = serde_json::from_str(&json).unwrap();
            assert_eq!(read, book, "{json}");
        }
    }

    fn sample_book() -> Book {
        Book {
            metadata: Metadata {
                title: Some("The Fixture Book".into()),
                author: Some("A. Author".into()),
                extra: [("language".to_string(), "en".to_string())]
                    .into_iter()
                    .collect(),
            },
            sections: vec![Section {
                id: NodeId::UNASSIGNED,
                source: Some("chapter-01.md".into()),
                title: Some("Chapter One".into()),
                blocks: vec![
                    Block::Heading {
                        id: NodeId::UNASSIGNED,
                        level: HeadingLevel::H1,
                        inlines: vec![text("Chapter One")],
                        position: Some(SourcePos { line: 1, column: 1 }),
                        span: span("# Chapter One\n"),
                    },
                    Block::Paragraph {
                        id: NodeId::UNASSIGNED,
                        inlines: vec![
                            text("It was the kind of morning that made you suspicious — too "),
                            Inline::Emphasis {
                                id: NodeId::UNASSIGNED,
                                children: vec![text("clean")],
                                position: None,
                                span: span("*clean*"),
                            },
                            text(", too quiet."),
                        ],
                        position: Some(SourcePos { line: 3, column: 1 }),
                        span: span(
                            "It was the kind of morning that made you suspicious — too *clean*, too quiet.\n",
                        ),
                    },
                    Block::Blockquote {
                        id: NodeId::UNASSIGNED,
                        blocks: vec![Block::Paragraph {
                            id: NodeId::UNASSIGNED,
                            inlines: vec![text("\"Nobody's early here.\"")],
                            position: None,
                            span: span("\"Nobody's early here.\"\n"),
                        }],
                        position: Some(SourcePos { line: 5, column: 1 }),
                        span: span("> \"Nobody's early here.\"\n"),
                    },
                    Block::ThematicBreak {
                        id: NodeId::UNASSIGNED,
                        position: Some(SourcePos { line: 7, column: 1 }),
                        span: span("---\n"),
                    },
                    Block::Image {
                        id: NodeId::UNASSIGNED,
                        url: "images/drawer.png".into(),
                        alt: "The drawer of knives".into(),
                        position: Some(SourcePos { line: 9, column: 1 }),
                        span: span("![The drawer of knives](images/drawer.png)"),
                    },
                ],
                position: Some(SourcePos { line: 1, column: 1 }),
                span: Some(SourceSpan {
                    start: 0,
                    end: SOURCE.len() as u32,
                }),
            }],
        }
    }

    /// Every id in the tree, in walk order — the order assignment uses.
    fn collect_ids(book: &Book) -> Vec<NodeId> {
        fn walk_block(ids: &mut Vec<NodeId>, block: &Block) {
            match block {
                Block::Heading { id, inlines, .. } | Block::Paragraph { id, inlines, .. } => {
                    ids.push(*id);
                    ids.extend(inlines.iter().flat_map(walk_inline_ids));
                }
                Block::Blockquote { id, blocks, .. } => {
                    ids.push(*id);
                    for nested in blocks {
                        walk_block(ids, nested);
                    }
                }
                Block::ThematicBreak { id, .. } | Block::Image { id, .. } => ids.push(*id),
            }
        }

        fn walk_inline_ids(inline: &Inline) -> Vec<NodeId> {
            match inline {
                Inline::Text { id, .. } | Inline::Code { id, .. } => vec![*id],
                Inline::Emphasis { id, children, .. }
                | Inline::Strong { id, children, .. }
                | Inline::Link { id, children, .. } => {
                    let mut ids = vec![*id];
                    ids.extend(children.iter().flat_map(walk_inline_ids));
                    ids
                }
            }
        }

        let mut ids = Vec::new();
        for section in &book.sections {
            ids.push(section.id);
            for block in &section.blocks {
                walk_block(&mut ids, block);
            }
        }
        ids
    }

    /// Two serializations of one tree are the same bytes: a dump is
    /// something to diff.
    #[test]
    fn serialization_is_stable() {
        let mut book = sample_book();
        book.assign_node_ids();
        let once = serde_json::to_string_pretty(&book).unwrap();
        assert_eq!(once, serde_json::to_string_pretty(&book).unwrap());
    }

    /// A serialized tree has no ids: the tree is authoritative,
    /// and identity is the engine's to hand out.
    #[test]
    fn ids_are_never_serialized() {
        let mut book = sample_book();
        book.assign_node_ids();
        let json = serde_json::to_string(&book).unwrap();
        assert!(!json.contains("\"id\""));
    }

    /// Tags are `type`, text runs are plain strings, and the shape
    /// maps onto mdast.
    #[test]
    fn a_serialized_tree_is_internally_tagged() {
        let block = Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![
                Inline::Text {
                    id: NodeId::UNASSIGNED,
                    value: "plain ".into(),
                    position: None,
                    span: None,
                },
                Inline::Strong {
                    id: NodeId::UNASSIGNED,
                    children: vec![Inline::Text {
                        id: NodeId::UNASSIGNED,
                        value: "bold".into(),
                        position: None,
                        span: None,
                    }],
                    position: None,
                    span: None,
                },
            ],
            position: Some(SourcePos { line: 4, column: 1 }),
            span: Some(SourceSpan { start: 40, end: 58 }),
        };
        assert_eq!(
            serde_json::to_value(&block).unwrap(),
            serde_json::json!({
                "type": "paragraph",
                "inlines": [
                    {"type": "text", "value": "plain "},
                    {"type": "strong", "children": [{"type": "text", "value": "bold"}]},
                ],
                "position": {"line": 4, "column": 1},
                "span": {"start": 40, "end": 58},
            }),
        );
    }

    /// A heading level is 1-6, and a level outside that is rejected
    /// rather than clamped.
    #[test]
    fn heading_levels_run_one_to_six() {
        for level in 1..=6u8 {
            let heading = HeadingLevel::try_from(level).expect("1-6 is a heading level");
            assert_eq!(u8::from(heading), level);
        }
        for outside in [0u8, 7, 255] {
            assert_eq!(
                HeadingLevel::try_from(outside),
                Err(InvalidHeadingLevel(outside)),
            );
        }
    }

    /// Ids are dense (exactly `1..=n`), assigned pre-order, and the
    /// same on every assignment.
    #[test]
    fn node_ids_are_dense_pre_order_and_deterministic() {
        let mut book = sample_book();
        book.assign_node_ids();
        let ids = collect_ids(&book);

        // Dense from 1: same length, same set, no gaps.
        let mut sorted = ids.clone();
        sorted.sort_by_key(|id| id.get());
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert_eq!(sorted.first().unwrap().get(), 1);
        assert_eq!(sorted.last().unwrap().get(), ids.len() as u32);

        // Pre-order: the section precedes its first block, a block
        // precedes its first inline, an inline precedes its children.
        let section = book.sections[0].id;
        let Block::Heading { id: heading, .. } = &book.sections[0].blocks[0] else {
            panic!("fixture starts with a heading");
        };
        let Block::Paragraph {
            id: paragraph,
            inlines,
            ..
        } = &book.sections[0].blocks[1]
        else {
            panic!("second block is a paragraph");
        };
        assert!(section.get() < heading.get());
        assert!(heading.get() < paragraph.get());
        let Inline::Emphasis {
            id: emphasis,
            children,
            ..
        } = &inlines[1]
        else {
            panic!("second inline is emphasis");
        };
        let Inline::Text { id: child, .. } = &children[0] else {
            panic!("emphasis child is text");
        };
        assert!(paragraph.get() < emphasis.get());
        assert!(emphasis.get() < child.get());

        // Deterministic: a fresh assignment over the same tree is
        // byte-identical.
        let first = collect_ids(&book);
        book.assign_node_ids();
        assert_eq!(first, collect_ids(&book));
    }

    /// A byte of the source answers with the node written there, and
    /// the innermost one: the run inside the emphasis rather than the
    /// emphasis, the emphasis rather than the paragraph.
    #[test]
    fn a_byte_answers_with_the_innermost_node_written_there() {
        let mut book = sample_book();
        book.assign_node_ids();

        let letter = SOURCE.find("clean").unwrap() as u32;
        let Some(Inline::Emphasis {
            id: emphasis,
            children,
            ..
        }) = paragraph(&book).get(1)
        else {
            panic!("the sample paragraph holds an emphasis");
        };
        assert_eq!(
            book.node_at("chapter-01.md", letter),
            Some(inline_id(&children[0]))
        );
        // The asterisk is markup, which no run was shaped from, so it
        // answers with the construct it opens.
        assert_eq!(book.node_at("chapter-01.md", letter - 1), Some(*emphasis));
        // A byte between two blocks belongs to no node under the
        // section, and the section is what covers it.
        let blank = SOURCE.find("\n\n").unwrap() as u32 + 1;
        assert_eq!(
            book.node_at("chapter-01.md", blank),
            Some(book.sections[0].id)
        );
        assert_eq!(book.node_at("chapter-01.md", SOURCE.len() as u32), None);
    }

    /// A question about one source is answered by that source's
    /// nodes: the same byte of two files is two different nodes, and
    /// a file the book has never read answers with nothing.
    #[test]
    fn a_question_is_answered_from_one_source() {
        let mut book = sample_book();
        let mut second = book.sections[0].clone();
        second.source = Some("chapter-02.md".into());
        book.sections.push(second);
        book.assign_node_ids();

        let letter = SOURCE.find("clean").unwrap() as u32;
        let first = book
            .node_at("chapter-01.md", letter)
            .expect("the first chapter");
        let second = book.node_at("chapter-02.md", letter).expect("the second");
        assert_ne!(first, second);
        assert_eq!(
            book.source_of(first).map(|(name, _)| name),
            Some("chapter-01.md")
        );
        assert_eq!(
            book.source_of(second).map(|(name, _)| name),
            Some("chapter-02.md")
        );
        assert_eq!(book.node_at("chapter-03.md", letter), None);
    }

    /// Every node of a parsed tree says where it was read from, and
    /// the byte it starts at answers with that node again.
    #[test]
    fn a_node_taken_to_its_source_and_back_is_the_same_node() {
        let mut book = sample_book();
        book.assign_node_ids();

        for id in collect_ids(&book) {
            let (source, span) = book
                .source_of(id)
                .expect("a parsed node was read from a file");
            assert_eq!(source, "chapter-01.md");
            assert!(
                span.end as usize <= SOURCE.len(),
                "node {} covers {span:?}, which is past the source",
                id.get(),
            );
            let there = book
                .node_at(source, span.start)
                .expect("a node was read there");
            let (_, back) = book.source_of(there).expect("and it says so");
            assert_eq!(back.start, span.start, "node {} starts elsewhere", id.get());
        }
    }

    /// A tree built rather than parsed was read from nothing, and
    /// both questions say so rather than guessing.
    #[test]
    fn a_tree_built_rather_than_parsed_answers_with_nothing() {
        let mut book = Book {
            metadata: Metadata::default(),
            sections: vec![Section {
                source: Some("chapter-01.md".into()),
                blocks: vec![Block::Paragraph {
                    id: NodeId::UNASSIGNED,
                    inlines: vec![Inline::Text {
                        id: NodeId::UNASSIGNED,
                        value: "Built by hand.".into(),
                        position: None,
                        span: None,
                    }],
                    position: None,
                    span: None,
                }],
                ..Section::default()
            }],
        };
        book.assign_node_ids();

        assert_eq!(book.node_at("chapter-01.md", 0), None);
        for id in collect_ids(&book) {
            assert_eq!(
                book.source_of(id),
                None,
                "node {} was read from nothing",
                id.get()
            );
        }
        assert_eq!(book.source_of(NodeId::UNASSIGNED), None);
    }

    /// The sample book's one paragraph of prose.
    fn paragraph(book: &Book) -> &[Inline] {
        let Block::Paragraph { inlines, .. } = &book.sections[0].blocks[1] else {
            panic!("the second block is a paragraph");
        };
        inlines
    }

    /// File + position in all four presence combinations.
    #[test]
    fn origin_formats_file_line_column() {
        let pos = SourcePos {
            line: 12,
            column: 3,
        };
        assert_eq!(
            origin(Some("chapter-01.md"), Some(pos)),
            "chapter-01.md:12:3"
        );
        assert_eq!(origin(Some("chapter-01.md"), None), "chapter-01.md");
        assert_eq!(origin(None, Some(pos)), "12:3");
        assert_eq!(origin(None, None), "");
    }
}
