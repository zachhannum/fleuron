//! The content tree: semantic input.
//!
//! Markdown frontends (Orca's remark/rehype pipeline) produce this; the
//! element vocabulary is bounded by what those frontends emit —
//! book/section, heading, paragraph, blockquote, thematic break,
//! emphasis/strong/code, image, link.
//!
//! This module is the **input contract**: everything downstream (style,
//! box construction, layout) consumes these types, and nothing widens
//! the vocabulary without a fixture and a test.
//!
//! # Wire format
//!
//! The content tree crosses every boundary as JSON, internally tagged
//! (`{"type": "paragraph", ...}`) so the shape maps one-to-one onto
//! mdast; a frontend serializes its tree with a field rename, not a
//! conversion pass. Postcard is the output wire (display list to the
//! WASM host), not this one.
//!
//! # Node identity
//!
//! `NodeId` is engine-assigned, never frontend-supplied: input can't
//! collide ids or forge diagnostic origins. Every node's `id` field is
//! `#[serde(skip)]` — ids never travel on the wire; the tree is
//! authoritative. Fresh off the wire every id is `NodeId::UNASSIGNED`;
//! `Book::assign_node_ids` assigns dense ids from 1 in document order
//! (pre-order: a node before its children, sections in reading order).
//!
//! # Source positions
//!
//! Every node carries an optional 1-based line/column into the markdown
//! source the frontend read it from; the section's `source` names the
//! file. `origin` formats the pair for diagnostics
//! (`chapter-01.md:12:3`). A missing position never fails a run.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Identity of one node in the content tree, for diagnostics and
/// incremental relayout.
///
/// Assigned in document order, starting at 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct NodeId(u32);

impl NodeId {
    /// What every node holds fresh off the wire, before assignment.
    pub const UNASSIGNED: NodeId = NodeId(0);

    /// The raw id. Monotonic in document order within one book.
    pub fn get(self) -> u32 {
        self.0
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Author, for the title page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Frontend-defined extensions (language, ISBN, subtitle…) keyed
    /// by name. Opaque to the engine; style reads them, layout
    /// doesn't.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

/// The root of the content tree: one book.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Book {
    /// The work's title, author and frontend extensions.
    pub metadata: Metadata,
    /// The chapters/files, in reading order.
    #[serde(default)]
    pub sections: Vec<Section>,
}

/// A chapter or file: the unit of markdown input and of source
/// attribution for diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Engine-assigned identity, for diagnostics; never on the wire.
    #[serde(skip)]
    pub id: NodeId,
    /// File the frontend read (e.g. `chapter-01.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Section title supplied outside the body (frontmatter
    /// `title:`); implies heading level 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The section's blocks, in reading order.
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// Where the frontend read this from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<SourcePos>,
}

/// A block-level element: the unit of fragmentation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// `#` through `######`; levels outside 1–6 are rejected at parse.
    Heading {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// `#` count, 1-6.
        level: HeadingLevel,
        /// The heading's text, in reading order.
        #[serde(default)]
        inlines: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// A run of prose: the unit line layout breaks.
    Paragraph {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// The paragraph's text, in reading order.
        #[serde(default)]
        inlines: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// A quotation set off by `>`; contents are blocks, not inlines —
    /// blockquotes nest.
    Blockquote {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// The quoted blocks, in reading order.
        #[serde(default)]
        blocks: Vec<Block>,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// `---`: a scene break, rendered as space or an ornament (❦).
    ThematicBreak {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// A block-level image.
    Image {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// Where the image lives; the host resolves it, not the engine.
        url: String,
        /// Alt text: not laid out, but part of the accessibility
        /// contract.
        #[serde(default)]
        alt: String,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
}

/// A heading level, 1–6, as markdown defines them.
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
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// The characters themselves, entities already decoded.
        value: String,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// `*emphasis*`: italic, in the default sheet.
    Emphasis {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// The emphasised inlines.
        #[serde(default)]
        children: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// `**strong**`: bold, in the default sheet.
    Strong {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// The strengthened inlines.
        #[serde(default)]
        children: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// `` `code` ``: monospace, and never hyphenated.
    Code {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// The literal code text; no markup inside.
        value: String,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// A hyperlink. The text lays out; the url is for painters that can carry one.
    Link {
        /// Engine-assigned identity, for diagnostics; never on the wire.
        #[serde(skip)]
        id: NodeId,
        /// The link target.
        url: String,
        /// The linked inlines.
        #[serde(default)]
        children: Vec<Inline>,
        /// Where the frontend read this from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
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

    /// Test-local shorthand: an unassigned text run.
    fn text(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            position: None,
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
                        inlines: vec![Inline::Text {
                            id: NodeId::UNASSIGNED,
                            value: "Chapter One".into(),
                            position: Some(SourcePos { line: 1, column: 1 }),
                        }],
                        position: Some(SourcePos { line: 1, column: 1 }),
                    },
                    Block::Paragraph {
                        id: NodeId::UNASSIGNED,
                        inlines: vec![
                            text("It was the kind of morning that made you suspicious — too "),
                            Inline::Emphasis {
                                id: NodeId::UNASSIGNED,
                                children: vec![text("clean")],
                                position: None,
                            },
                            text(", too quiet."),
                        ],
                        position: Some(SourcePos { line: 3, column: 1 }),
                    },
                    Block::Blockquote {
                        id: NodeId::UNASSIGNED,
                        blocks: vec![Block::Paragraph {
                            id: NodeId::UNASSIGNED,
                            inlines: vec![text("\"Nobody's early here.\"")],
                            position: None,
                        }],
                        position: Some(SourcePos { line: 5, column: 1 }),
                    },
                    Block::ThematicBreak {
                        id: NodeId::UNASSIGNED,
                        position: Some(SourcePos { line: 7, column: 1 }),
                    },
                    Block::Image {
                        id: NodeId::UNASSIGNED,
                        url: "images/drawer.png".into(),
                        alt: "The drawer of knives".into(),
                        position: Some(SourcePos { line: 9, column: 1 }),
                    },
                ],
                position: Some(SourcePos { line: 1, column: 1 }),
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

    /// The tree survives a JSON round-trip unchanged.
    #[test]
    fn json_round_trip() {
        let book = sample_book();
        let json = serde_json::to_string_pretty(&book).unwrap();
        let back: Book = serde_json::from_str(&json).unwrap();
        assert_eq!(book, back);
    }

    /// A serialized tree carries no ids; a deserialized one comes back
    /// unassigned.
    #[test]
    fn ids_never_travel_on_the_wire() {
        let mut book = sample_book();
        book.assign_node_ids();
        let json = serde_json::to_string(&book).unwrap();
        assert!(!json.contains("\"id\""));
        let back: Book = serde_json::from_str(&json).unwrap();
        assert!(
            collect_ids(&back)
                .iter()
                .all(|id| *id == NodeId::UNASSIGNED)
        );
    }

    /// The checked-in fixture is valid against this schema.
    #[test]
    fn fixture_deserializes() {
        let text = include_str!("../../../fixtures/book.json");
        let book: Book = serde_json::from_str(text).expect("fixture book.json parses");
        assert_eq!(book.metadata.title.as_deref(), Some("Gulliver's Travels"));
        assert_eq!(book.sections.len(), 1);
        let blocks: Vec<&Block> = book.sections.iter().flat_map(|s| s.blocks.iter()).collect();
        assert!(blocks.iter().any(|b| matches!(b, Block::Heading { .. })));
        let quote = blocks
            .iter()
            .find_map(|b| match b {
                Block::Blockquote { blocks, .. } => Some(blocks.len()),
                _ => None,
            })
            .expect("fixture has a blockquote");
        assert!(quote >= 2, "fixture blockquote has multiple paragraphs");
        // Chapter argument paragraphs open with an emphasis run.
        let italic_lead = blocks.iter().any(|b| matches!(b, Block::Paragraph { inlines, .. } if matches!(inlines.first(), Some(Inline::Emphasis { .. }))));
        assert!(italic_lead);
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

    /// Tags are `type`, text runs are plain strings.
    #[test]
    fn internally_tagged_json_parses() {
        let json = r#"{
            "type": "paragraph",
            "inlines": [
                {"type": "text", "value": "plain "},
                {"type": "strong", "children": [{"type": "text", "value": "bold"}]},
                {"type": "code", "value": "code"}
            ],
            "position": {"line": 4, "column": 1}
        }"#;
        let block: Block = serde_json::from_str(json).unwrap();
        let Block::Paragraph {
            inlines, position, ..
        } = &block
        else {
            panic!("expected paragraph");
        };
        assert_eq!(inlines.len(), 3);
        assert_eq!(*position, Some(SourcePos { line: 4, column: 1 }));
    }

    /// Levels outside 1–6 fail at parse rather than clamping.
    #[test]
    fn heading_level_out_of_range_fails() {
        let json = r#"{"type": "heading", "level": 7, "inlines": []}"#;
        let err = serde_json::from_str::<Block>(json).unwrap_err();
        assert!(err.to_string().contains("1-6"), "got: {err}");
    }

    /// Every optional field can be omitted.
    #[test]
    fn missing_fields_default() {
        let json = r#"{"type": "paragraph"}"#;
        let block: Block = serde_json::from_str(json).unwrap();
        let Block::Paragraph {
            inlines, position, ..
        } = &block
        else {
            panic!("expected paragraph");
        };
        assert!(inlines.is_empty());
        assert!(position.is_none());
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
