//! Markdown events to blocks, and the mapping's degradations.

use fleuron::Warning;
use fleuron::content::{
    Block, HeadingLevel, Inline, Section, SourcePos, origin, text as inline_text,
};
use pulldown_cmark::{Event, Options as ParserOptions, Parser, Tag, TagEnd};

use crate::Options;

/// Reads one source into sections and diagnostics.
pub fn run(text: &str, source: &str, options: &Options) -> (Vec<Section>, Vec<Warning>) {
    let mut converter = Converter::new(text, source, options);
    for (event, range) in Parser::new_ext(text, parser_options(options)).into_offset_iter() {
        converter.event(event, range.start);
    }
    converter.finish()
}

/// The dialect, as the parser wants it.
fn parser_options(options: &Options) -> ParserOptions {
    let dialect = options.dialect;
    let mut parser = ParserOptions::empty();
    parser.set(
        ParserOptions::ENABLE_YAML_STYLE_METADATA_BLOCKS,
        dialect.frontmatter,
    );
    parser.set(ParserOptions::ENABLE_GFM, dialect.gfm);
    parser.set(ParserOptions::ENABLE_TABLES, dialect.gfm);
    parser.set(ParserOptions::ENABLE_STRIKETHROUGH, dialect.gfm);
    parser.set(ParserOptions::ENABLE_TASKLISTS, dialect.gfm);
    parser.set(ParserOptions::ENABLE_WIKILINKS, dialect.wikilinks);
    parser.set(
        ParserOptions::ENABLE_SMART_PUNCTUATION,
        dialect.smart_punctuation,
    );
    parser
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
            // Columns count bytes. This is diagnostic data, never
            // layout input.
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
    Link {
        url: String,
    },
    Image {
        url: String,
    },
    /// Markup with no counterpart in the vocabulary: the children
    /// fold into the parent unwrapped.
    Plain,
}

struct Converter<'a> {
    source: &'a str,
    options: &'a Options,
    lines: LineIndex,
    sections: Vec<Section>,
    warnings: Vec<Warning>,
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
    /// Depth of metadata blocks, whose text is not content.
    metadata: u32,
}

impl<'a> Converter<'a> {
    fn new(text: &'a str, source: &'a str, options: &'a Options) -> Converter<'a> {
        Converter {
            source,
            options,
            lines: LineIndex::new(text),
            sections: Vec::new(),
            warnings: Vec::new(),
            blocks: vec![Vec::new()],
            inlines: Vec::new(),
            deferred: Vec::new(),
            metadata: 0,
        }
    }

    fn event(&mut self, event: Event<'_>, offset: usize) {
        let at = self.lines.position(offset);
        match event {
            Event::Start(Tag::MetadataBlock(_)) => self.metadata += 1,
            Event::End(TagEnd::MetadataBlock(_)) => self.metadata -= 1,

            Event::Start(Tag::Paragraph) => self.push_inlines(InlineFor::Paragraph, at),
            Event::Start(Tag::Heading { level, .. }) => {
                self.push_inlines(InlineFor::Heading(heading_level(level)), at)
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.degrades("an inline image", "a block of its own", at);
                self.push_inlines(
                    InlineFor::Image {
                        url: dest_url.into_string(),
                    },
                    at,
                )
            }
            Event::Start(Tag::Emphasis) => self.push_inlines(InlineFor::Emphasis, at),
            Event::Start(Tag::Strong) => self.push_inlines(InlineFor::Strong, at),
            Event::Start(Tag::Link { dest_url, .. }) => self.push_inlines(
                InlineFor::Link {
                    url: dest_url.into_string(),
                },
                at,
            ),
            Event::Start(Tag::Strikethrough) => {
                self.degrades("strikethrough", "plain text", at);
                self.push_inlines(InlineFor::Plain, at)
            }
            Event::Start(Tag::Superscript) => {
                self.degrades("a superscript", "plain text", at);
                self.push_inlines(InlineFor::Plain, at)
            }
            Event::Start(Tag::Subscript) => {
                self.degrades("a subscript", "plain text", at);
                self.push_inlines(InlineFor::Plain, at)
            }
            Event::Start(Tag::BlockQuote(_)) => self.blocks.push(Vec::new()),

            Event::Start(Tag::List(_)) => self.degrades("a list", "one paragraph per item", at),
            Event::Start(Tag::Table(_)) => self.degrades("a table", "one paragraph per cell", at),
            Event::Start(Tag::CodeBlock(_)) => {
                self.degrades("a code block", "a paragraph", at);
                self.push_inlines(InlineFor::Paragraph, at)
            }
            Event::Start(Tag::FootnoteDefinition(_)) => {
                self.degrades("a footnote", "prose where it was written", at)
            }
            Event::Start(Tag::DefinitionList) => {
                self.degrades("a definition list", "one paragraph per entry", at)
            }
            Event::Start(Tag::HtmlBlock) => self.drops("an html block", at),
            // A tight list item holds its text directly, with no
            // paragraph around it. The frame catches that text; a
            // loose item's own paragraph closes first and leaves this
            // one empty.
            Event::Start(
                Tag::Item
                | Tag::TableCell
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition,
            ) => self.push_inlines(InlineFor::Paragraph, at),

            Event::End(
                TagEnd::Paragraph
                | TagEnd::CodeBlock
                | TagEnd::Item
                | TagEnd::TableCell
                | TagEnd::DefinitionListTitle
                | TagEnd::DefinitionListDefinition
                | TagEnd::Heading(_)
                | TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Superscript
                | TagEnd::Subscript
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
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                self.degrades("math", "plain text", at);
                self.text(&math, at)
            }
            Event::Html(_) | Event::InlineHtml(_) => self.drops("html", at),
            Event::FootnoteReference(_) => self.drops("a footnote reference", at),
            Event::TaskListMarker(_) => self.drops("a task list marker", at),
            // A wrapped line is a space; the shaper never sees the
            // markdown's ragged column.
            Event::SoftBreak | Event::HardBreak => self.text(" ", at),
            Event::Rule => self.rule(at),
            Event::End(_) | Event::Start(_) => {}
        }
    }

    /// Reports a construct the vocabulary has no room for, naming
    /// what it becomes instead.
    fn degrades(&mut self, what: &str, becomes: &str, at: SourcePos) {
        self.warn(format!("{what} is set as {becomes}"), at);
    }

    /// Reports a construct that carries no prose, and so leaves
    /// nothing behind.
    fn drops(&mut self, what: &str, at: SourcePos) {
        self.warn(format!("{what} has no counterpart and is dropped"), at);
    }

    fn warn(&mut self, message: String, at: SourcePos) {
        self.warnings.push(Warning {
            message,
            origin: Some(origin(Some(self.source), Some(at))),
        });
    }

    /// Opens an inline frame. Frames nest: emphasis inside a paragraph
    /// collects into its own and folds back on close.
    fn push_inlines(&mut self, kind: InlineFor, at: SourcePos) {
        self.inlines.push((Vec::new(), kind, at));
    }

    fn text(&mut self, value: &str, at: SourcePos) {
        if value.is_empty() || self.metadata > 0 {
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
    /// files that block instead, so a heading may open a section and a
    /// paragraph joins the one already open.
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
            InlineFor::Plain => {
                for child in children {
                    self.inline(child);
                }
            }
            // The content vocabulary has no inline image: the image
            // becomes a block, deferred until the paragraph that held
            // it closes, and its alt text stays with it.
            InlineFor::Image { url } => self.deferred.push(Block::Image {
                id: Default::default(),
                url,
                alt: inline_text(&children),
                position: Some(at),
            }),
            InlineFor::Heading(level) => {
                if self.options.sections.opens(level) {
                    self.open_section(at);
                }
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

    fn rule(&mut self, at: SourcePos) {
        self.push_block(Block::ThematicBreak {
            id: Default::default(),
            position: Some(at),
        });
    }

    /// Starts a section at a heading, closing the one before it. Any
    /// still-open block frames belong to the section being closed: a
    /// heading inside a blockquote nests oddly rather than losing its
    /// prose.
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

    /// Files a block into the innermost open frame, opening a section
    /// for content that precedes the first one.
    fn push_block(&mut self, block: Block) {
        if self.sections.is_empty() {
            let position = block_position(&block);
            self.sections.push(Section {
                id: Default::default(),
                source: Some(self.source.to_string()),
                title: None,
                blocks: Vec::new(),
                position,
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

    fn finish(mut self) -> (Vec<Section>, Vec<Warning>) {
        self.flush_section();
        (self.sections, self.warnings)
    }
}

fn block_position(block: &Block) -> Option<SourcePos> {
    match block {
        Block::Heading { position, .. }
        | Block::Paragraph { position, .. }
        | Block::Blockquote { position, .. }
        | Block::ThematicBreak { position, .. }
        | Block::Image { position, .. } => *position,
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
    use crate::{Dialect, Sections, to_sections};

    fn read(markdown: &str) -> Vec<Section> {
        to_sections(markdown, "test.md", &Options::default()).0
    }

    fn text_of(block: &Block) -> String {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                inline_text(inlines)
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn a_heading_at_the_policy_level_opens_a_section() {
        let sections = read("# One\n\nA.\n\n# Two\n\nB.\n\n## Under two\n\nC.\n");
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[1].blocks.len(), 4, "the h2 stayed in its section");
        assert_eq!(sections[0].source.as_deref(), Some("test.md"));
    }

    #[test]
    fn a_whole_source_is_one_section() {
        let (sections, _) = to_sections(
            "# One\n\nA.\n\n# Two\n\nB.\n",
            "chapter-01.md",
            &Options {
                sections: Sections::Whole,
                ..Options::default()
            },
        );
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].blocks.len(), 4);
        assert_eq!(sections[0].position, Some(SourcePos { line: 1, column: 1 }));
    }

    #[test]
    fn prose_before_the_first_heading_gets_a_section_of_its_own() {
        let sections = read("Front matter.\n\n# One\n\nA.\n");
        assert_eq!(sections.len(), 2);
        assert_eq!(text_of(&sections[0].blocks[0]), "Front matter.");
        assert_eq!(sections[0].position, Some(SourcePos { line: 1, column: 1 }));
    }

    #[test]
    fn emphasis_nests_under_the_paragraph_that_holds_it() {
        let sections = read("# C\n\nPlain _stressed_ plain.\n");
        let Block::Paragraph { inlines, .. } = &sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        assert!(matches!(inlines[1], Inline::Emphasis { .. }));
        assert_eq!(inline_text(inlines), "Plain stressed plain.");
    }

    #[test]
    fn wrapped_lines_join_with_a_space() {
        let sections = read("# C\n\none\ntwo\n");
        assert_eq!(text_of(&sections[0].blocks[1]), "one two");
    }

    #[test]
    fn blockquotes_keep_their_blocks() {
        let sections = read("# C\n\n> Quoted.\n>\n> Still quoted.\n");
        let Block::Blockquote { blocks, .. } = &sections[0].blocks[1] else {
            panic!("expected a blockquote");
        };
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn a_rule_in_the_body_is_a_scene_break() {
        let sections = read("# C\n\nA.\n\n---\n\nB.\n");
        assert!(matches!(sections[0].blocks[2], Block::ThematicBreak { .. }));
    }

    #[test]
    fn frontmatter_is_metadata_rather_than_content() {
        let sections = read("---\ntitle: A Book\n---\n\n# C\n\nProse.\n");
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].blocks.len(), 2);
        assert_eq!(text_of(&sections[0].blocks[0]), "C");
    }

    #[test]
    fn positions_are_one_based_lines_into_the_source() {
        let sections = read("# C\n\nProse.\n");
        let Block::Heading { position, .. } = &sections[0].blocks[0] else {
            panic!("expected a heading");
        };
        assert_eq!(*position, Some(SourcePos { line: 1, column: 1 }));
        let Block::Paragraph { position, .. } = &sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        assert_eq!(*position, Some(SourcePos { line: 3, column: 1 }));
    }

    #[test]
    fn ids_are_left_for_assembly_to_number() {
        let sections = read("# C\n\nProse.\n");
        assert_eq!(sections[0].id, fleuron::content::NodeId::UNASSIGNED);
    }

    /// The three constructs a manuscript most often reaches for that
    /// the vocabulary has no room for. Each says where it was
    /// written, and each leaves its prose behind.
    #[test]
    fn lists_code_blocks_and_tables_warn_and_keep_their_prose() {
        let markdown = "\
# C

- one
- two

```
code line
```

| a | b |
|---|---|
| c | d |
";
        let (sections, warnings) = to_sections(
            markdown,
            "test.md",
            &Options {
                dialect: Dialect::gfm(),
                ..Options::default()
            },
        );
        let reported: Vec<(&str, &str)> = warnings
            .iter()
            .map(|w| (w.message.as_str(), w.origin.as_deref().unwrap()))
            .collect();
        assert_eq!(
            reported,
            [
                ("a list is set as one paragraph per item", "test.md:3:1"),
                ("a code block is set as a paragraph", "test.md:6:1"),
                ("a table is set as one paragraph per cell", "test.md:10:1"),
            ],
        );
        let prose: Vec<String> = sections[0].blocks[1..].iter().map(text_of).collect();
        assert_eq!(prose, ["one", "two", "code line\n", "a", "b", "c", "d"]);
    }

    /// Obsidian's departures are switches, not a second mapping.
    #[test]
    fn a_dialect_decides_what_the_parser_recognises() {
        let markdown = "# C\n\nSee [[Another Note]].\n";
        let plain = read(markdown);
        assert_eq!(text_of(&plain[0].blocks[1]), "See [[Another Note]].");

        let (obsidian, _) = to_sections(
            markdown,
            "test.md",
            &Options {
                dialect: Dialect::obsidian(),
                ..Options::default()
            },
        );
        let Block::Paragraph { inlines, .. } = &obsidian[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        assert!(
            inlines.iter().any(|i| matches!(i, Inline::Link { .. })),
            "{inlines:?}",
        );
    }

    #[test]
    fn an_inline_image_becomes_a_block_after_the_paragraph_that_held_it() {
        let (sections, warnings) = to_sections(
            "# C\n\nProse ![a plate](plate.png) more.\n",
            "test.md",
            &Options::default(),
        );
        assert_eq!(text_of(&sections[0].blocks[1]), "Prose  more.");
        let Block::Image { url, alt, .. } = &sections[0].blocks[2] else {
            panic!("expected an image block");
        };
        assert_eq!((url.as_str(), alt.as_str()), ("plate.png", "a plate"));
        assert_eq!(warnings.len(), 1);
    }
}
