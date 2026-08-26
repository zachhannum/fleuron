//! Markdown to content tree, for fixtures only.
//!
//! The engine has no markdown frontend and gains none here: this
//! stands in for the remark/rehype pipeline so the harness can measure
//! real books without hand-authoring megabytes of JSON.
//!
//! Two conventions the vendored corpus follows, and this reads:
//!
//! - The preamble — headings of the form `Key: Value` before the
//!   book's first thematic break — is metadata, not content.
//! - Every heading after it opens a section. Chapters are the unit of
//!   fragmentation, so they are the unit the tree is cut into.
//!
//! Constructs the content vocabulary has no room for degrade rather
//! than vanish: list items and code blocks flow through as their own
//! paragraphs. Text is never dropped — a fixture that quietly loses
//! prose measures the wrong book.

use fleuron::content::{Block, Book, HeadingLevel, Inline, Metadata, Section, SourcePos};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Parses markdown into a content tree with node ids assigned.
///
/// `source` becomes every section's `source`, as a frontend's file
/// name would.
pub fn to_book(markdown: &str, source: &str) -> Book {
    let mut converter = Converter::new(markdown, source);
    for (event, range) in Parser::new_ext(markdown, Options::empty()).into_offset_iter() {
        converter.event(event, range.start);
    }
    let mut book = converter.finish();
    book.assign_node_ids();
    book
}

/// Byte offset to 1-based line and column, for source positions.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> LineIndex {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        LineIndex { starts }
    }

    fn position(&self, offset: usize) -> SourcePos {
        let line = self.starts.partition_point(|start| *start <= offset).max(1);
        SourcePos {
            line: line as u32,
            // Columns count bytes: the corpus is ASCII, and this is
            // diagnostic data, never layout input.
            column: (offset - self.starts[line - 1] + 1) as u32,
        }
    }
}

/// What an inline frame is collecting for.
enum InlineFor {
    Paragraph,
    Heading(HeadingLevel),
    Emphasis,
    Strong,
    Link { url: String },
    Image { url: String },
}

struct Converter<'a> {
    source: &'a str,
    lines: LineIndex,
    metadata: Metadata,
    /// True until the thematic break that closes the preamble.
    preamble: bool,
    sections: Vec<Section>,
    /// Block frames, innermost last: the outermost is the current
    /// section's body, each nested one a blockquote under
    /// construction.
    blocks: Vec<Vec<Block>>,
    /// Inline frames, innermost last, with what each is collecting for
    /// and where it started.
    inlines: Vec<(Vec<Inline>, InlineFor, SourcePos)>,
    /// Blocks an inline construct produced, flushed once the block
    /// that contained it closes.
    deferred: Vec<Block>,
}

impl<'a> Converter<'a> {
    fn new(markdown: &'a str, source: &'a str) -> Converter<'a> {
        Converter {
            source,
            lines: LineIndex::new(markdown),
            metadata: Metadata::default(),
            preamble: true,
            sections: Vec::new(),
            blocks: vec![Vec::new()],
            inlines: Vec::new(),
            deferred: Vec::new(),
        }
    }

    fn event(&mut self, event: Event<'_>, offset: usize) {
        let at = self.lines.position(offset);
        match event {
            Event::Start(Tag::Paragraph) => self.push_inlines(InlineFor::Paragraph, at),
            Event::Start(Tag::Heading { level, .. }) => {
                self.push_inlines(InlineFor::Heading(heading_level(level)), at)
            }
            Event::Start(Tag::Image { dest_url, .. }) => self.push_inlines(
                InlineFor::Image {
                    url: dest_url.into_string(),
                },
                at,
            ),
            Event::Start(Tag::Emphasis) => self.push_inlines(InlineFor::Emphasis, at),
            Event::Start(Tag::Strong) => self.push_inlines(InlineFor::Strong, at),
            Event::Start(Tag::Link { dest_url, .. }) => self.push_inlines(
                InlineFor::Link {
                    url: dest_url.into_string(),
                },
                at,
            ),
            Event::Start(Tag::BlockQuote(_)) => self.blocks.push(Vec::new()),
            // A tight list item holds its text directly, with no
            // paragraph around it. The frame catches that text; a
            // loose item's own paragraph closes first and leaves this
            // one empty.
            Event::Start(Tag::Item | Tag::CodeBlock(_)) => {
                self.push_inlines(InlineFor::Paragraph, at)
            }

            Event::End(
                TagEnd::Paragraph
                | TagEnd::CodeBlock
                | TagEnd::Item
                | TagEnd::Heading(_)
                | TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Link
                | TagEnd::Image,
            ) => self.close_inlines(),
            Event::End(TagEnd::BlockQuote(_)) => self.close_blockquote(at),

            Event::Text(text) => self.text(&text, at),
            Event::Code(code) => self.inline(Inline::Code {
                id: Default::default(),
                value: code.into_string(),
                position: Some(at),
            }),
            // A wrapped line is a space; the shaper never sees the
            // markdown's ragged column.
            Event::SoftBreak | Event::HardBreak => self.text(" ", at),
            Event::Rule => self.rule(at),
            _ => {}
        }
    }

    /// Opens an inline frame. Frames nest: emphasis inside a paragraph
    /// collects into its own and folds back on close.
    fn push_inlines(&mut self, kind: InlineFor, at: SourcePos) {
        self.inlines.push((Vec::new(), kind, at));
    }

    fn text(&mut self, value: &str, at: SourcePos) {
        if value.is_empty() {
            return;
        }
        self.inline(Inline::Text {
            id: Default::default(),
            value: value.to_string(),
            position: Some(at),
        });
    }

    /// Appends to the innermost inline frame. Text outside any block
    /// has nowhere to go and is dropped; the parser does not emit it.
    fn inline(&mut self, inline: Inline) {
        if let Some((frame, ..)) = self.inlines.last_mut() {
            frame.push(inline);
        }
    }

    /// Closes the innermost inline frame. Nested markup folds into
    /// its parent as one node; a frame a block was collecting into
    /// files that block instead — a heading opens a section, a
    /// paragraph joins the current one, and preamble headings become
    /// metadata rather than content.
    fn close_inlines(&mut self) {
        let Some((children, kind, at)) = self.inlines.pop() else {
            return;
        };
        match kind {
            InlineFor::Emphasis => self.inline(Inline::Emphasis {
                id: Default::default(),
                children,
                position: Some(at),
            }),
            InlineFor::Strong => self.inline(Inline::Strong {
                id: Default::default(),
                children,
                position: Some(at),
            }),
            InlineFor::Link { url } => self.inline(Inline::Link {
                id: Default::default(),
                url,
                children,
                position: Some(at),
            }),
            // The content vocabulary has no inline image: the image
            // becomes a block, deferred until the paragraph that held
            // it closes, and its alt text stays with it.
            InlineFor::Image { url } => self.deferred.push(Block::Image {
                id: Default::default(),
                url,
                alt: flatten(&children),
                position: Some(at),
            }),
            InlineFor::Heading(level) => {
                if self.preamble {
                    self.metadata_from(&flatten(&children));
                    return;
                }
                self.open_section(at);
                self.push_block(Block::Heading {
                    id: Default::default(),
                    level,
                    inlines: children,
                    position: Some(at),
                });
                self.flush_deferred();
            }
            InlineFor::Paragraph => {
                if !children.is_empty() {
                    self.push_block(Block::Paragraph {
                        id: Default::default(),
                        inlines: children,
                        position: Some(at),
                    });
                }
                self.flush_deferred();
            }
        }
    }

    /// Files the blocks inline constructs produced, now that the
    /// block that contained them has closed.
    fn flush_deferred(&mut self) {
        let deferred = std::mem::take(&mut self.deferred);
        for block in deferred {
            self.push_block(block);
        }
    }

    fn close_blockquote(&mut self, at: SourcePos) {
        let Some(blocks) = self.blocks.pop() else {
            return;
        };
        if blocks.is_empty() {
            return;
        }
        self.push_block(Block::Blockquote {
            id: Default::default(),
            blocks,
            position: Some(at),
        });
    }

    /// The first thematic break closes the preamble; later ones are
    /// scene breaks.
    fn rule(&mut self, at: SourcePos) {
        if self.preamble {
            self.preamble = false;
            return;
        }
        self.push_block(Block::ThematicBreak {
            id: Default::default(),
            position: Some(at),
        });
    }

    /// `Title: …` and `Author: …` are named fields; anything else in
    /// the preamble is a frontend extension.
    fn metadata_from(&mut self, heading: &str) {
        let Some((key, value)) = heading.split_once(':') else {
            return;
        };
        let value = value.trim().to_string();
        match key.trim().to_ascii_lowercase().as_str() {
            "title" => self.metadata.title = Some(value),
            "author" => self.metadata.author = Some(value),
            other => {
                self.metadata.extra.insert(other.to_string(), value);
            }
        }
    }

    /// Starts a section at a heading, closing the one before it. Any
    /// still-open block frames belong to the section being closed: no
    /// book in the corpus puts a heading inside a blockquote, and one
    /// that did would nest oddly rather than lose its prose.
    fn open_section(&mut self, at: SourcePos) {
        self.flush_section();
        self.sections.push(Section {
            id: Default::default(),
            source: Some(self.source.to_string()),
            title: None,
            blocks: Vec::new(),
            position: Some(at),
        });
    }

    /// Files a block into the innermost open frame, opening an
    /// untitled section for content that precedes the first heading.
    fn push_block(&mut self, block: Block) {
        if self.sections.is_empty() {
            self.sections.push(Section {
                id: Default::default(),
                source: Some(self.source.to_string()),
                title: None,
                blocks: Vec::new(),
                position: None,
            });
        }
        match self.blocks.last_mut() {
            Some(frame) => frame.push(block),
            None => self.blocks.push(vec![block]),
        }
    }

    /// Moves the accumulated blocks onto the section they belong to
    /// and reopens an empty frame for the next one.
    fn flush_section(&mut self) {
        let blocks: Vec<Block> = self.blocks.drain(..).flatten().collect();
        self.blocks.push(Vec::new());
        if let Some(section) = self.sections.last_mut() {
            section.blocks.extend(blocks);
        }
    }

    fn finish(mut self) -> Book {
        self.flush_section();
        Book {
            metadata: self.metadata,
            sections: self.sections,
        }
    }
}

/// The text of an inline tree, markup discarded.
pub(crate) fn flatten(inlines: &[Inline]) -> String {
    let mut text = String::new();
    push_text(inlines, &mut text);
    text
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

fn heading_level(level: pulldown_cmark::HeadingLevel) -> HeadingLevel {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => HeadingLevel::H1,
        H2 => HeadingLevel::H2,
        H3 => HeadingLevel::H3,
        H4 => HeadingLevel::H4,
        H5 => HeadingLevel::H5,
        H6 => HeadingLevel::H6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREAMBLE: &str = "# Title: A Book\n\n## Author: Someone\n\n## Year: 1900\n\n-------\n\n";

    fn book(body: &str) -> Book {
        to_book(&format!("{PREAMBLE}{body}"), "test.md")
    }

    #[test]
    fn the_preamble_becomes_metadata_not_content() {
        let book = book("## Chapter I\n\nProse.\n");
        assert_eq!(book.metadata.title.as_deref(), Some("A Book"));
        assert_eq!(book.metadata.author.as_deref(), Some("Someone"));
        assert_eq!(
            book.metadata.extra.get("year").map(String::as_str),
            Some("1900")
        );
        assert_eq!(book.sections.len(), 1);
    }

    #[test]
    fn every_heading_opens_a_section() {
        let book = book("## One\n\nA.\n\n## Two\n\nB.\n\n### Three\n\nC.\n");
        assert_eq!(book.sections.len(), 3);
        for section in &book.sections {
            assert_eq!(section.blocks.len(), 2, "heading plus its prose");
        }
    }

    #[test]
    fn emphasis_nests_under_the_paragraph_that_holds_it() {
        let book = book("## C\n\nPlain _stressed_ plain.\n");
        let Block::Paragraph { inlines, .. } = &book.sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        assert!(matches!(inlines[1], Inline::Emphasis { .. }));
        assert_eq!(flatten(inlines), "Plain stressed plain.");
    }

    #[test]
    fn wrapped_lines_join_with_a_space() {
        let book = book("## C\n\none\ntwo\n");
        let Block::Paragraph { inlines, .. } = &book.sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        assert_eq!(flatten(inlines), "one two");
    }

    #[test]
    fn blockquotes_keep_their_blocks() {
        let book = book("## C\n\n> Quoted.\n>\n> Still quoted.\n");
        let Block::Blockquote { blocks, .. } = &book.sections[0].blocks[1] else {
            panic!("expected a blockquote");
        };
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn a_later_rule_is_a_scene_break() {
        let book = book("## C\n\nA.\n\n---\n\nB.\n");
        assert!(matches!(
            book.sections[0].blocks[2],
            Block::ThematicBreak { .. }
        ));
    }

    #[test]
    fn list_items_flow_through_as_blocks() {
        let book = book("## C\n\n- one\n- two\n");
        let text: Vec<String> = book.sections[0].blocks[1..]
            .iter()
            .map(|b| match b {
                Block::Paragraph { inlines, .. } => flatten(inlines),
                _ => panic!("expected paragraphs"),
            })
            .collect();
        assert_eq!(text, ["one", "two"]);
    }

    #[test]
    fn positions_are_one_based_lines_into_the_source() {
        let book = to_book("# Title: T\n\n-------\n\n## C\n\nProse.\n", "test.md");
        let Block::Heading { position, .. } = &book.sections[0].blocks[0] else {
            panic!("expected a heading");
        };
        assert_eq!(*position, Some(SourcePos { line: 5, column: 1 }));
    }

    #[test]
    fn node_ids_are_assigned() {
        let book = book("## C\n\nProse.\n");
        assert_ne!(book.sections[0].id, fleuron::content::NodeId::UNASSIGNED);
    }
}
