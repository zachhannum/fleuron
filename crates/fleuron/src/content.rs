//! The content tree: semantic input.
//!
//! Markdown frontends (Orca's remark/rehype pipeline) produce this; the
//! element vocabulary is bounded by what those frontends emit —
//! book/section, heading, paragraph, blockquote, thematic break,
//! emphasis/strong/code, image, link. Schema and fixtures: see the
//! Foundations milestone on the issue tracker.
//!
//! This module is the **input contract**: everything downstream (style,
//! box construction, layout) consumes these types, and nothing
//! downstream may widen them without a fixture and a test.
//!
//! # Wire format (decision)
//!
//! The content tree speaks JSON, only JSON. Enums are internally
//! tagged (`{"type": "paragraph", ...}`) so the shape maps one-to-one
//! onto mdast — Orca serializes its remark tree with a field rename,
//! not a conversion pass — and that single representation is the
//! contract at every edge (CLI file input, WASM host message).
//!
//! It deliberately does not cross as postcard: serde's internal
//! tagging buffers through `deserialize_any`, which non-self-describing
//! formats don't implement. Postcard is the *output* wire (display
//! list to the WASM host, where compactness pays per keystroke); the
//! content tree crosses a boundary once per book, where mdast
//! compatibility is worth more than a compact encoding. If a binary
//! input wire is ever wanted, that requires custom `Deserialize`
//! impls — a breaking change to revisit, not a default to bake in.
//!
//! # Node identity (decision)
//!
//! [`NodeId`] is engine-assigned, not frontend-supplied: untrusted
//! input can't collide ids or forge diagnostic origins. Every node
//! carries an `id` field that is `#[serde(skip)]` — ids never travel
//! on the wire; the tree is authoritative. Fresh off the wire every
//! id is [`NodeId::UNASSIGNED`], and the engine calls
//! [`Book::assign_node_ids`] exactly once before any downstream pass.
//! Ids are dense from 1 in document order (pre-order: a node before
//! its children, sections in reading order), so diagnostics can order
//! by id and incremental relayout can diff by it.
//!
//! # Source positions (decision)
//!
//! Every node carries an optional 1-based line/column pointing at the
//! markdown source the frontend read it from; the section's `source`
//! names the file. [`origin`] formats the pair the way
//! `Warning::origin` prints it (`chapter-01.md:12:3`). Frontends that
//! don't track positions (hand-built content, tests) omit them; a
//! missing position never fails a run.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Identity of one node in the content tree, for diagnostics and
/// (later) incremental relayout.
///
/// Assigned by [`Book::assign_node_ids`] in document order, starting
/// at 1. Zero never appears in an assigned tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct NodeId(u32);

impl NodeId {
    /// The pre-assignment value: what every node holds fresh off the
    /// wire, before [`Book::assign_node_ids`] runs.
    pub const UNASSIGNED: NodeId = NodeId(0);

    /// The raw id. Monotonic in document order within one [`Book`].
    pub fn get(self) -> u32 {
        self.0
    }
}

/// A 1-based position in the frontend's source document.
///
/// Line and column are as the markdown parser reported them (an mdast
/// `position.start` copies straight over). This is diagnostic data,
/// never layout input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePos {
    /// 1-based line in the source markdown.
    pub line: u32,
    /// 1-based column in the source markdown.
    pub column: u32,
}

/// Formats a section's file name plus a node position the way
/// diagnostics print it: `chapter-01.md:12:3`. Degrades to the bare
/// file name, bare position, or empty string — a missing position
/// never fails a run.
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
    /// by name. The engine treats these as opaque; style can read
    /// them, layout never does.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

/// The root of the content tree: one book.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub metadata: Metadata,
    /// The chapters/files, in reading order. Empty is legal (a blank
    /// book still produces front matter) but never produced by a real
    /// frontend.
    #[serde(default)]
    pub sections: Vec<Section>,
}

/// A chapter or file: the unit of markdown input and of source
/// attribution for diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    #[serde(skip)]
    pub id: NodeId,
    /// File the frontend read (e.g. `chapter-01.md`); pairs with node
    /// positions to form `Warning::origin`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Section title when the frontend supplies one outside the body
    /// (frontmatter `title:`). Where set, heading level 1 is implied;
    /// a heading in the body is the frontend's problem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<SourcePos>,
}

/// A block-level element: the unit of fragmentation input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// `#` through `######`; levels outside 1–6 are rejected at parse.
    Heading {
        #[serde(skip)]
        id: NodeId,
        level: HeadingLevel,
        #[serde(default)]
        inlines: Vec<Inline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    Paragraph {
        #[serde(skip)]
        id: NodeId,
        #[serde(default)]
        inlines: Vec<Inline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// A quotation set off by `>`; contents are blocks, not inlines —
    /// blockquotes nest.
    Blockquote {
        #[serde(skip)]
        id: NodeId,
        #[serde(default)]
        blocks: Vec<Block>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// `---`: a scene break, rendered as space or an ornament (❦).
    ThematicBreak {
        #[serde(skip)]
        id: NodeId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    /// Block-level image for now; inline images wait for a real book
    /// that needs them.
    Image {
        #[serde(skip)]
        id: NodeId,
        url: String,
        /// Alt text: not laid out, but part of the accessibility
        /// contract and the PDF's structure tree later.
        #[serde(default)]
        alt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
}

/// A heading level, 1–6, as markdown defines them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum HeadingLevel {
    H1,
    H2,
    H3,
    H4,
    H5,
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
        #[serde(skip)]
        id: NodeId,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    Emphasis {
        #[serde(skip)]
        id: NodeId,
        #[serde(default)]
        children: Vec<Inline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    Strong {
        #[serde(skip)]
        id: NodeId,
        #[serde(default)]
        children: Vec<Inline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    Code {
        #[serde(skip)]
        id: NodeId,
        /// The literal code text; no markup inside.
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
    Link {
        #[serde(skip)]
        id: NodeId,
        url: String,
        #[serde(default)]
        children: Vec<Inline>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<SourcePos>,
    },
}

impl Book {
    /// Assign fresh [`NodeId`]s to every node, in document order
    /// (pre-order: a node before its children, sections in reading
    /// order), starting at 1.
    ///
    /// The engine calls this exactly once, after deserialization and
    /// before any downstream pass. Calling it again is legal but
    /// renumbers — downstream state keyed by id (warnings, relayout
    /// caches) belongs to one assignment.
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

    /// Every id in the tree, in walk order — the same order
    /// `assign_node_ids` assigns in.
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

    /// Acceptance: types round-trip through JSON — the one wire format
    /// the content tree has (see the module docs for why not postcard).
    #[test]
    fn json_round_trip() {
        let book = sample_book();
        let json = serde_json::to_string_pretty(&book).unwrap();
        let back: Book = serde_json::from_str(&json).unwrap();
        assert_eq!(book, back);
    }

    /// Ids are `#[serde(skip)]`: the wire never carries them, and a
    /// deserialized tree comes back unassigned.
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
        assert_eq!(book.metadata.title.as_deref(), Some("The Fixture Book"));
        assert_eq!(book.sections.len(), 2);
        // The fixture exercises more than paragraphs: at least one
        // heading, blockquote, and thematic break across the book.
        let blocks: Vec<&Block> = book.sections.iter().flat_map(|s| s.blocks.iter()).collect();
        assert!(blocks.iter().any(|b| matches!(b, Block::Heading { .. })));
        assert!(blocks.iter().any(|b| matches!(b, Block::Blockquote { .. })));
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, Block::ThematicBreak { .. }))
        );
    }

    /// Node ids are dense (exactly `1..=n`), assigned pre-order, and
    /// deterministic across assignments — the scheme diagnostics and
    /// relayout rely on.
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

    /// mdast-shaped input parses: tag is `type`, text runs are plain.
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

    /// Heading levels outside 1–6 are a parse error, not a silent
    /// clamp — the bounded vocabulary is enforced at the boundary.
    #[test]
    fn heading_level_out_of_range_fails() {
        let json = r#"{"type": "heading", "level": 7, "inlines": []}"#;
        let err = serde_json::from_str::<Block>(json).unwrap_err();
        assert!(err.to_string().contains("1-6"), "got: {err}");
    }

    /// Frontends that don't track positions omit them; nothing that
    /// builds a tree by hand should have to write `null`s.
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

    /// `Warning::origin` formatting: file + position, and the
    /// degrade-to-file / degrade-to-nothing cases.
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
