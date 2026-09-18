//! Markdown events to blocks, and the mapping's degradations.
//!
//! An attribute line is a paragraph whose whole content is one brace
//! run, so it is an event to match rather than a tail of prose to
//! scan, and it reaches every block in the vocabulary. It is held
//! until the block under it arrives; a line that names nothing is
//! prose again, and warns.

use std::ops::Range;

use fleuron::Warning;
use fleuron::content::{
    Alignment, Attributes, Block, Cell, HeadingLevel, Inline, ListItem, Row, Section, SourcePos,
    SourceSpan, block_position, block_span, inline_position, inline_span, origin,
    text as inline_text,
};
use pulldown_cmark::{Event, Options as ParserOptions, Parser, Tag, TagEnd};

use crate::{Options, Sections};

/// Both attribute diagnostics name the same two forms, so they are
/// written once.
const UNSUPPORTED_ATTRIBUTE: &str = "Unsupported attribute. Only `.class` or `#id` are valid.";
const UNSUPPORTED_HEADING_ATTRIBUTE: &str =
    "Unsupported heading attribute. Only `.class` or `#id` are valid.";

/// Reads one source into sections and diagnostics.
pub fn run(text: &str, source: &str, options: &Options) -> (Vec<Section>, Vec<Warning>) {
    let mut converter = Converter::new(text, source, options);
    for (event, range) in Parser::new_ext(text, parser_options(options)).into_offset_iter() {
        converter.event(event, range);
    }
    let (mut sections, warnings) = converter.finish();
    // A source read whole is one chapter, so its frontmatter is that
    // chapter's: `title:` names it, and `class:` and `id:` are what a
    // sheet reaches it by. A source cut at headings is many, and the
    // headings name them.
    if options.sections == Sections::Whole
        && options.dialect.frontmatter
        && let [section] = sections.as_mut_slice()
    {
        let front = crate::frontmatter(text);
        section.title = front.title;
        section.attributes = Attributes {
            id: front.extra.get("id").cloned(),
            classes: front
                .extra
                .get("class")
                .map(|names| names.split_whitespace().map(String::from).collect())
                .unwrap_or_default(),
        };
    }
    (sections, warnings)
}

/// The dialect, translated for the parser.
fn parser_options(options: &Options) -> ParserOptions {
    let dialect = options.dialect;
    let mut parser = ParserOptions::empty();
    parser.set(
        ParserOptions::ENABLE_YAML_STYLE_METADATA_BLOCKS,
        dialect.frontmatter,
    );
    parser.set(ParserOptions::ENABLE_GFM, dialect.gfm);
    parser.set(ParserOptions::ENABLE_TABLES, dialect.tables);
    parser.set(ParserOptions::ENABLE_STRIKETHROUGH, dialect.gfm);
    parser.set(ParserOptions::ENABLE_TASKLISTS, dialect.gfm);
    parser.set(ParserOptions::ENABLE_WIKILINKS, dialect.wikilinks);
    parser.set(ParserOptions::ENABLE_HEADING_ATTRIBUTES, dialect.attributes);
    parser.set(
        ParserOptions::ENABLE_SMART_PUNCTUATION,
        dialect.smart_punctuation,
    );
    parser
}

/// Where one event was read from: the position a diagnostic quotes,
/// and the bytes of the source it covers. A start tag covers the
/// whole construct it opens, so a node's span holds the span of every
/// node written inside it.
#[derive(Debug, Clone, Copy)]
struct Read {
    position: SourcePos,
    span: SourceSpan,
}

/// The source these blocks were read from, together.
fn extent(blocks: &[Block]) -> Option<SourceSpan> {
    let spans = || blocks.iter().filter_map(block_span);
    Some(SourceSpan {
        start: spans().map(|span| span.start).min()?,
        end: spans().map(|span| span.end).max()?,
    })
}

/// Where a run of inlines was read from, together.
fn inline_extent(inlines: &[Inline]) -> Option<Read> {
    let spans = || inlines.iter().filter_map(inline_span);
    Some(Read {
        position: inlines.iter().find_map(inline_position)?,
        span: SourceSpan {
            start: spans().map(|span| span.start).min()?,
            end: spans().map(|span| span.end).max()?,
        },
    })
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

/// An attribute line, held until the block it names arrives.
///
/// The inlines it was read as come with it, because a line that
/// names nothing is prose again and prose is never dropped.
struct Pending {
    attributes: Attributes,
    inlines: Vec<Inline>,
    read: Read,
}

/// What an inline frame is collecting for.
enum InlineFor {
    Paragraph,
    /// A heading, with what its own trailing brace run named.
    Heading(HeadingLevel, Attributes),
    Emphasis,
    Strong,
    Link {
        url: String,
    },
    Image {
        url: String,
    },
    /// A cell of the table being read.
    Cell,
    /// The text an item of a tight list holds directly, with no
    /// paragraph written around it.
    Item,
    /// Markup with no counterpart in the vocabulary: the children
    /// fold into the parent unwrapped.
    Plain,
}

/// A list while its items are still arriving.
struct ListFrame {
    read: Read,
    ordered: bool,
    start: u32,
    /// Whether no item has held a paragraph of its own yet.
    tight: bool,
    items: Vec<ListItem>,
    /// The item whose blocks are arriving, and where it was read from.
    item: Option<Read>,
    /// How many block frames are open while the blocks of that item
    /// arrive.
    depth: usize,
    /// The attribute line held above the list, which names the list
    /// rather than its first item.
    held: Option<Pending>,
}

/// A table while its rows are still arriving.
struct TableFrame {
    read: Read,
    /// The alignment the delimiter row wrote on each column.
    columns: Vec<Option<Alignment>>,
    head: Vec<Row>,
    body: Vec<Row>,
    /// Whether the rows arriving are header rows.
    in_head: bool,
    /// The row whose cells are arriving, and where it was read from.
    row: Option<(Vec<Cell>, Read)>,
}

struct Converter<'a> {
    /// The markdown being read, for the constructs whose shape is in
    /// the source rather than in the events.
    text: &'a str,
    /// The file the source is named by, for diagnostics.
    source: &'a str,
    options: &'a Options,
    lines: LineIndex,
    sections: Vec<Section>,
    warnings: Vec<Warning>,
    /// Block frames, innermost last: the outermost is the current
    /// section's body, each nested one a blockquote or a list item
    /// under construction.
    blocks: Vec<Vec<Block>>,
    /// Inline frames, innermost last, with what each is collecting
    /// for and where it was read from.
    inlines: Vec<(Vec<Inline>, InlineFor, Read)>,
    /// Blocks an inline construct produced, flushed once the block
    /// that contained it closes.
    deferred: Vec<Block>,
    /// The attribute line waiting for the block it names.
    pending: Option<Pending>,
    /// One held attribute line per open blockquote: the line before a
    /// quote names the quote, and the blocks inside it are their own.
    quoted: Vec<Option<Pending>>,
    /// The lists being read, innermost last.
    lists: Vec<ListFrame>,
    /// The table being read. A cell holds only inlines, so tables do
    /// not nest.
    table: Option<TableFrame>,
    /// Depth of metadata blocks, whose text is not content.
    metadata: u32,
}

impl<'a> Converter<'a> {
    fn new(text: &'a str, source: &'a str, options: &'a Options) -> Converter<'a> {
        Converter {
            text,
            source,
            options,
            lines: LineIndex::new(text),
            sections: Vec::new(),
            warnings: Vec::new(),
            blocks: vec![Vec::new()],
            inlines: Vec::new(),
            deferred: Vec::new(),
            pending: None,
            quoted: Vec::new(),
            lists: Vec::new(),
            table: None,
            metadata: 0,
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        let read = self.read(range);
        let at = read.position;
        if matches!(
            event,
            Event::Start(
                Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::BlockQuote(_)
                    | Tag::CodeBlock(_)
                    | Tag::List(_)
                    | Tag::Table(_)
                    | Tag::HtmlBlock
                    | Tag::FootnoteDefinition(_)
                    | Tag::DefinitionList
            ) | Event::Rule
        ) {
            self.settle_item();
        }
        match event {
            Event::Start(Tag::MetadataBlock(_)) => self.metadata += 1,
            Event::End(TagEnd::MetadataBlock(_)) => self.metadata -= 1,

            Event::Start(Tag::Paragraph) => {
                self.loosen();
                self.push_inlines(InlineFor::Paragraph, read)
            }
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => {
                let spanned = self.heading_run(read).is_some();
                if !attrs.is_empty() && !spanned {
                    self.warn(UNSUPPORTED_HEADING_ATTRIBUTE, at);
                }
                let named = match spanned {
                    true => Attributes::default(),
                    false => Attributes {
                        id: id.map(|id| id.into_string()),
                        classes: classes.into_iter().map(|c| c.into_string()).collect(),
                    },
                };
                self.push_inlines(InlineFor::Heading(heading_level(level), named), read)
            }
            Event::Start(Tag::Image { dest_url, .. }) => self.push_inlines(
                InlineFor::Image {
                    url: dest_url.into_string(),
                },
                read,
            ),
            Event::Start(Tag::Emphasis) => self.push_inlines(InlineFor::Emphasis, read),
            Event::Start(Tag::Strong) => self.push_inlines(InlineFor::Strong, read),
            Event::Start(Tag::Link { dest_url, .. }) => self.push_inlines(
                InlineFor::Link {
                    url: dest_url.into_string(),
                },
                read,
            ),
            Event::Start(Tag::Strikethrough) => {
                self.warn(
                    "Strikethrough is not supported. Falling back to plain text.",
                    at,
                );
                self.push_inlines(InlineFor::Plain, read)
            }
            Event::Start(Tag::Superscript) => {
                self.warn(
                    "Superscript is not supported. Falling back to plain text.",
                    at,
                );
                self.push_inlines(InlineFor::Plain, read)
            }
            Event::Start(Tag::Subscript) => {
                self.warn(
                    "Subscript is not supported. Falling back to plain text.",
                    at,
                );
                self.push_inlines(InlineFor::Plain, read)
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.quoted.push(self.pending.take());
                self.blocks.push(Vec::new());
            }

            Event::Start(Tag::List(first)) => self.lists.push(ListFrame {
                read,
                ordered: first.is_some(),
                start: first.map_or(1, |number| u32::try_from(number).unwrap_or(u32::MAX)),
                tight: true,
                items: Vec::new(),
                item: None,
                depth: 0,
                held: self.pending.take(),
            }),
            Event::Start(Tag::Item) => self.open_item(read),
            Event::End(TagEnd::Item) => self.close_item(),
            Event::End(TagEnd::List(_)) => self.close_list(read),
            Event::Start(Tag::Table(columns)) => {
                self.table = Some(TableFrame {
                    read,
                    columns: columns.iter().map(alignment).collect(),
                    head: Vec::new(),
                    body: Vec::new(),
                    in_head: false,
                    row: None,
                })
            }
            Event::Start(Tag::TableHead) => {
                if let Some(table) = self.table.as_mut() {
                    table.in_head = true;
                    table.row = Some((Vec::new(), read));
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(table) = self.table.as_mut() {
                    table.row = Some((Vec::new(), read));
                }
            }
            Event::Start(Tag::TableCell) => self.push_inlines(InlineFor::Cell, read),
            Event::End(TagEnd::TableRow) => self.close_row(),
            Event::End(TagEnd::TableHead) => {
                self.close_row();
                if let Some(table) = self.table.as_mut() {
                    table.in_head = false;
                }
            }
            Event::End(TagEnd::Table) => self.close_table(),
            Event::Start(Tag::CodeBlock(_)) => {
                self.warn(
                    "Code blocks are not supported. Falling back to a plain paragraph.",
                    at,
                );
                self.push_inlines(InlineFor::Paragraph, read)
            }
            Event::Start(Tag::FootnoteDefinition(_)) => self.warn(
                "Footnotes are not supported. The note is kept where it was written.",
                at,
            ),
            Event::Start(Tag::DefinitionList) => self.warn(
                "Definition lists are not supported. Falling back to one paragraph per entry.",
                at,
            ),
            Event::Start(Tag::HtmlBlock) => {
                self.warn("HTML blocks are not supported and will be ignored.", at)
            }
            Event::Start(Tag::DefinitionListTitle | Tag::DefinitionListDefinition) => {
                self.push_inlines(InlineFor::Paragraph, read)
            }

            Event::End(
                TagEnd::Paragraph
                | TagEnd::CodeBlock
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
            Event::End(TagEnd::BlockQuote(_)) => self.close_blockquote(read),

            Event::Text(text) => self.text(&text, read),
            Event::Code(code) => self.inline(Inline::Code {
                id: Default::default(),
                value: code.into_string(),
                attributes: Attributes::default(),
                position: Some(at),
                span: Some(read.span),
            }),
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                self.warn("Math is not supported. Falling back to plain text.", at);
                self.text(&math, read)
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                self.warn("Inline HTML is not supported and will be ignored.", at)
            }
            Event::FootnoteReference(_) => self.warn(
                "Footnote references are not supported and will be ignored.",
                at,
            ),
            Event::TaskListMarker(_) => self.warn(
                "Task list markers are not supported and will be ignored.",
                at,
            ),
            // A wrapped line is a space; the shaper never sees the
            // markdown's ragged column.
            Event::SoftBreak => self.text(" ", read),
            Event::HardBreak => self.inline(Inline::Break {
                id: Default::default(),
                attributes: Attributes::default(),
                position: Some(at),
                span: Some(read.span),
            }),
            Event::Rule => self.rule(read),
            Event::End(_) => {}
        }
    }

    fn read(&self, range: Range<usize>) -> Read {
        Read {
            position: self.lines.position(range.start),
            span: SourceSpan {
                start: range.start as u32,
                end: range.end as u32,
            },
        }
    }

    fn warn(&mut self, message: impl Into<String>, at: SourcePos) {
        self.warnings.push(Warning {
            message: message.into(),
            origin: Some(origin(Some(self.source), Some(at))),
        });
    }

    /// Opens an inline frame. Frames nest: emphasis inside a paragraph
    /// collects into its own and folds back on close.
    fn push_inlines(&mut self, kind: InlineFor, read: Read) {
        self.inlines.push((Vec::new(), kind, read));
    }

    fn text(&mut self, value: &str, read: Read) {
        if value.is_empty() || self.metadata > 0 {
            return;
        }
        self.inline(Inline::Text {
            id: Default::default(),
            value: value.to_string(),
            attributes: Attributes::default(),
            position: Some(read.position),
            span: Some(read.span),
        });
    }

    /// Appends to the innermost inline frame. Text directly inside an
    /// item of a tight list opens a frame of its own. Text outside any
    /// block has nowhere to go and is dropped; the parser does not
    /// emit it.
    fn inline(&mut self, inline: Inline) {
        if self.inlines.is_empty()
            && self.in_item()
            && let Some(read) = inline_extent(std::slice::from_ref(&inline))
        {
            self.push_inlines(InlineFor::Item, read);
        }
        if let Some((frame, ..)) = self.inlines.last_mut() {
            frame.push(inline);
        }
    }

    /// Closes the innermost inline frame. Nested markup folds into
    /// its parent as one node; a frame a block was collecting into
    /// files that block instead, so a heading may open a section and a
    /// paragraph joins the one already open.
    fn close_inlines(&mut self) {
        let Some((mut children, kind, read)) = self.inlines.pop() else {
            return;
        };
        if matches!(kind, InlineFor::Heading(..))
            && let Some(run) = self.heading_run(read)
        {
            children.push(Inline::Text {
                id: Default::default(),
                value: self.text[run.clone()].to_string(),
                attributes: Attributes::default(),
                position: Some(self.lines.position(run.start)),
                span: Some(SourceSpan {
                    start: run.start as u32,
                    end: run.end as u32,
                }),
            });
        }
        let children = self.fold_spans(children);
        let (at, span) = (Some(read.position), Some(read.span));
        match kind {
            InlineFor::Emphasis => self.inline(Inline::Emphasis {
                id: Default::default(),
                children,
                attributes: Attributes::default(),
                position: at,
                span,
            }),
            InlineFor::Strong => self.inline(Inline::Strong {
                id: Default::default(),
                children,
                attributes: Attributes::default(),
                position: at,
                span,
            }),
            InlineFor::Link { url } => self.inline(Inline::Link {
                id: Default::default(),
                url,
                children,
                attributes: Attributes::default(),
                position: at,
                span,
            }),
            InlineFor::Plain => {
                for child in children {
                    self.inline(child);
                }
            }
            // An image written in a cell stays in the cell, after the
            // prose it was written in.
            InlineFor::Cell => {
                self.displaced(&children);
                let mut blocks = Vec::new();
                if !children.is_empty() {
                    blocks.push(Block::Paragraph {
                        id: Default::default(),
                        inlines: children,
                        attributes: Attributes::default(),
                        position: at,
                        span,
                    });
                }
                blocks.append(&mut self.deferred);
                self.cell(blocks, read);
            }
            // The content vocabulary has no inline image: the image
            // becomes a block, deferred until the paragraph it was
            // written in closes, and its alt text stays with it.
            InlineFor::Image { url } => self.deferred.push(Block::Image {
                id: Default::default(),
                url,
                alt: inline_text(&children),
                attributes: Attributes::default(),
                position: at,
                span,
            }),
            // A brace run over a line of dashes is a setext heading
            // in CommonMark, and an empty heading is not what was
            // written: it is an attribute line over a scene break.
            InlineFor::Heading(_, named)
                if children.is_empty() && !named.is_empty() && self.underlined(read) =>
            {
                self.push_block(Block::ThematicBreak {
                    id: Default::default(),
                    attributes: named,
                    position: Some(self.dashes_at(read)),
                    span,
                });
                self.flush_deferred();
            }
            InlineFor::Heading(level, named) => {
                self.displaced(&children);
                // The line above the heading is taken before the
                // section opens, so the heading that opens one is
                // still named by it.
                let line = self.pending.take();
                if self.options.sections.opens(level) {
                    self.open_section(read.position);
                }
                let mut heading = Block::Heading {
                    id: Default::default(),
                    level,
                    inlines: children,
                    attributes: named,
                    position: at,
                    span,
                };
                if let Some(line) = line {
                    annotate(&mut heading, line);
                }
                self.push_block(heading);
                self.flush_deferred();
            }
            InlineFor::Paragraph => self.paragraph(children, read),
            // The item's span covers its marker and whatever is nested
            // under it, so the paragraph takes the span of its words.
            InlineFor::Item => {
                let read = inline_extent(&children).unwrap_or(read);
                self.paragraph(children, read)
            }
        }
    }

    /// Files a paragraph whose inlines have all arrived.
    fn paragraph(&mut self, children: Vec<Inline>, read: Read) {
        if let Some(children) = self.brace_run(children, read) {
            self.displaced(&children);
            if let Some(forced) = self.forced_break(&children, read) {
                self.push_block(forced);
            } else if !children.is_empty() {
                self.push_block(Block::Paragraph {
                    id: Default::default(),
                    inlines: children,
                    attributes: Attributes::default(),
                    position: Some(read.position),
                    span: Some(read.span),
                });
            }
        }
        self.flush_deferred();
    }

    /// The break a paragraph is when its whole text is `\pagebreak`
    /// or `\columnbreak`.
    fn forced_break(&self, children: &[Inline], read: Read) -> Option<Block> {
        if !self.options.dialect.breaks {
            return None;
        }
        let mut written = String::new();
        for inline in children {
            let Inline::Text { value, .. } = inline else {
                return None;
            };
            written.push_str(value);
        }
        let (id, attributes) = (Default::default(), Attributes::default());
        let (position, span) = (Some(read.position), Some(read.span));
        match written.trim() {
            "\\pagebreak" => Some(Block::PageBreak {
                id,
                attributes,
                position,
                span,
            }),
            "\\columnbreak" => Some(Block::ColumnBreak {
                id,
                attributes,
                position,
                span,
            }),
            _ => None,
        }
    }

    /// Whether the blocks arriving are directly inside a list item,
    /// rather than inside a block the item holds.
    fn in_item(&self) -> bool {
        self.lists
            .last()
            .is_some_and(|list| list.item.is_some() && self.blocks.len() == list.depth)
    }

    /// Marks the list loose: an item holds a paragraph of its own.
    fn loosen(&mut self) {
        if self.in_item()
            && let Some(list) = self.lists.last_mut()
        {
            list.tight = false;
        }
    }

    /// Files the text a tight item holds directly, once a block inside
    /// the item follows it.
    fn settle_item(&mut self) {
        if matches!(self.inlines.last(), Some((_, InlineFor::Item, _))) {
            self.close_inlines();
        }
    }

    /// Opens an item of the innermost list.
    fn open_item(&mut self, read: Read) {
        let Some(list) = self.lists.last_mut() else {
            return;
        };
        list.item = Some(read);
        self.blocks.push(Vec::new());
        list.depth = self.blocks.len();
    }

    /// Files the item whose blocks have all arrived. A line at the end
    /// of the item names nothing.
    fn close_item(&mut self) {
        self.settle_item();
        self.flush_deferred();
        self.dangling();
        let Some(read) = self.lists.last_mut().and_then(|list| list.item.take()) else {
            return;
        };
        let blocks = self.blocks.pop().unwrap_or_default();
        if let Some(list) = self.lists.last_mut() {
            list.items.push(ListItem {
                id: Default::default(),
                blocks,
                attributes: Attributes::default(),
                position: Some(read.position),
                span: Some(read.span),
            });
        }
    }

    /// Files the list whose items have all arrived. It takes the names
    /// of the attribute line above it, as any block does.
    fn close_list(&mut self, end: Read) {
        let Some(list) = self.lists.pop() else {
            return;
        };
        self.pending = list.held;
        self.push_block(Block::List {
            id: Default::default(),
            ordered: list.ordered,
            start: list.start,
            tight: list.tight,
            items: list.items,
            attributes: Attributes::default(),
            position: Some(list.read.position),
            span: Some(SourceSpan {
                start: list.read.span.start,
                end: list.read.span.end.max(end.span.end),
            }),
        });
    }

    /// Whether a heading was written as a line of text underlined by
    /// dashes, which is how `---` under an attribute line parses.
    fn underlined(&self, read: Read) -> bool {
        self.options.dialect.attributes && self.dashes(read).is_some()
    }

    /// The dashes of an underlined heading, as an offset into the
    /// source.
    fn dashes(&self, read: Read) -> Option<usize> {
        let text = &self.text[read.span.start as usize..read.span.end as usize];
        let under = text.trim_end().rfind('\n')? + 1;
        let dashes = text[under..].trim_end();
        (dashes.len() >= 3 && dashes.chars().all(|c| c == '-')).then_some(under)
    }

    /// Where those dashes were written.
    fn dashes_at(&self, read: Read) -> SourcePos {
        let under = self.dashes(read).unwrap_or_default();
        self.lines.position(read.span.start as usize + under)
    }

    /// A paragraph that is one brace run and nothing else is a name
    /// for another block: the image it was written after, or the
    /// block written under it. Everything else is the prose it was
    /// read as, handed back to be filed.
    fn brace_run(&mut self, children: Vec<Inline>, read: Read) -> Option<Vec<Inline>> {
        if !self.options.dialect.attributes {
            return Some(children);
        }
        let read_as = match brace_text(&children) {
            None => return Some(children),
            Some(inside) => named(inside),
        };
        let Some(attributes) = read_as else {
            self.warn(UNSUPPORTED_ATTRIBUTE, read.position);
            return Some(children);
        };
        // An image is a block written inline, so a run after one
        // alone on its line is that block's.
        if let [Block::Image { .. }] = self.deferred.as_slice() {
            let image = &mut self.deferred[0];
            let (mine, span) = slots(image);
            merge(mine, attributes);
            if let Some(span) = span {
                span.end = span.end.max(read.span.end);
            }
            return None;
        }
        if !self.deferred.is_empty() {
            return Some(children);
        }
        self.dangling();
        self.pending = Some(Pending {
            attributes,
            inlines: children,
            read,
        });
        None
    }

    /// Folds every `[run]{.class}` of one inline frame into a span.
    ///
    /// A bracketed run with no destination arrives as a bracket of
    /// its own, the run, a second bracket, and the text after it, so
    /// a span is read out of what a frame collected rather than out
    /// of one event. The brace run has to follow the bracket
    /// directly: `[words] {.class}` is prose.
    fn fold_spans(&mut self, mut children: Vec<Inline>) -> Vec<Inline> {
        if !self.options.dialect.attributes {
            return children;
        }
        let mut at = 2;
        while at < children.len() {
            let Some((inside, rest, moved)) = brace_head(&children[at]) else {
                at += 1;
                continue;
            };
            let (inside, rest) = (inside.to_string(), rest.to_string());
            if !bracket(&children[at - 1], ']') {
                at += 1;
                continue;
            }
            let Some(open) = children[..at - 1]
                .iter()
                .rposition(|inline| bracket(inline, '['))
            else {
                at += 1;
                continue;
            };
            let Some(attributes) = named(&inside) else {
                if let Some(position) = inline_position(&children[at]) {
                    self.warn(UNSUPPORTED_ATTRIBUTE, position);
                }
                at += 1;
                continue;
            };
            let position = inline_position(&children[open]);
            let span = inline_span(&children[open]).map(|opened| SourceSpan {
                start: opened.start,
                end: moved.map_or(opened.end, |moved| moved.start),
            });
            let inner: Vec<Inline> = children.drain(open + 1..at - 1).collect();
            let mut folded = vec![Inline::Span {
                id: Default::default(),
                children: inner,
                attributes,
                position,
                span,
            }];
            if !rest.is_empty() {
                folded.push(Inline::Text {
                    id: Default::default(),
                    value: rest,
                    attributes: Attributes::default(),
                    position: moved.map(|moved| self.lines.position(moved.start as usize)),
                    span: moved,
                });
            }
            at = open + folded.len();
            // The bracket, the bracket after it, and the brace run.
            children.splice(open..open + 3, folded);
        }
        children
    }

    /// The trailing brace run of a heading that closes a bracketed
    /// run, as bytes of the source.
    ///
    /// The parser takes the run at the end of a heading line as the
    /// heading's own names. A run directly after `]` belongs to the
    /// span it closes, and the heading hands it back.
    fn heading_run(&self, read: Read) -> Option<Range<usize>> {
        if !self.options.dialect.attributes {
            return None;
        }
        let start = read.span.start as usize;
        let end = (read.span.end as usize).min(self.text.len());
        let line = self.text.get(start..end)?.trim_end();
        if !line.ends_with('}') {
            return None;
        }
        let open = line.rfind('{')?;
        line[..open]
            .ends_with(']')
            .then(|| start + open..start + line.len())
    }

    /// Files an attribute line that named nothing as the prose it was
    /// read as.
    fn dangling(&mut self) {
        let Some(line) = self.pending.take() else {
            return;
        };
        self.warn(
            "Attribute line with no block under it. Falling back to plain text.",
            line.read.position,
        );
        self.push_block(Block::Paragraph {
            id: Default::default(),
            inlines: line.inlines,
            attributes: Attributes::default(),
            position: Some(line.read.position),
            span: Some(line.read.span),
        });
    }

    /// Reports the images a block is about to be broken around.
    ///
    /// An image is a block in the vocabulary and inline in markdown,
    /// so one written among prose is set after the prose it was
    /// written in, which is a move worth reporting. An image written on a
    /// line of its own displaces nothing, and reporting every image
    /// in the book would be noise.
    fn displaced(&mut self, siblings: &[Inline]) {
        if siblings.is_empty() {
            return;
        }
        let moved: Vec<SourcePos> = self
            .deferred
            .iter()
            .filter_map(|block| match block {
                Block::Image { position, .. } => *position,
                _ => None,
            })
            .collect();
        for at in moved {
            self.warn(
                "Inline images become blocks. The image is placed after the paragraph it was \
                 written in.",
                at,
            );
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

    fn close_blockquote(&mut self, read: Read) {
        // A line at the end of the quote is inside it, and names
        // nothing; the line before the quote was held when it opened.
        self.dangling();
        let Some(blocks) = self.blocks.pop() else {
            return;
        };
        self.pending = self.quoted.pop().flatten();
        if blocks.is_empty() {
            return;
        }
        self.push_block(Block::Blockquote {
            id: Default::default(),
            blocks,
            attributes: Attributes::default(),
            position: Some(read.position),
            span: Some(read.span),
        });
    }

    /// Files one cell in the row being read, under the alignment its
    /// column was written with. A cell with no row to go in is prose,
    /// which is never dropped.
    fn cell(&mut self, blocks: Vec<Block>, read: Read) {
        let Some((table, cells)) = self
            .table
            .as_mut()
            .and_then(|table| Some((&table.columns, &mut table.row.as_mut()?.0)))
        else {
            for block in blocks {
                self.push_block(block);
            }
            return;
        };
        let align = table.get(cells.len()).copied().flatten();
        cells.push(Cell {
            id: Default::default(),
            blocks,
            align,
            attributes: Attributes::default(),
            position: Some(read.position),
            span: Some(read.span),
        });
    }

    /// Files the row whose cells have all arrived.
    fn close_row(&mut self) {
        let Some(table) = self.table.as_mut() else {
            return;
        };
        let Some((cells, read)) = table.row.take() else {
            return;
        };
        let row = Row {
            id: Default::default(),
            cells,
            attributes: Attributes::default(),
            position: Some(read.position),
            span: Some(read.span),
        };
        if table.in_head {
            table.head.push(row);
        } else {
            table.body.push(row);
        }
    }

    /// Files the table whose rows have all arrived. It takes the
    /// names of the attribute line above it, as any block does.
    fn close_table(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        self.push_block(Block::Table {
            id: Default::default(),
            head: table.head,
            body: table.body,
            attributes: Attributes::default(),
            position: Some(table.read.position),
            span: Some(table.read.span),
        });
    }

    fn rule(&mut self, read: Read) {
        self.push_block(Block::ThematicBreak {
            id: Default::default(),
            attributes: Attributes::default(),
            position: Some(read.position),
            span: Some(read.span),
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
            attributes: Attributes::default(),
            blocks: Vec::new(),
            position: Some(at),
            span: None,
        });
    }

    /// Files a block into the innermost open frame, opening a section
    /// for content that precedes the first one. The block takes the
    /// names of the attribute line above it, if one was held.
    fn push_block(&mut self, mut block: Block) {
        if let Some(line) = self.pending.take() {
            annotate(&mut block, line);
        }
        if self.sections.is_empty() {
            let position = block_position(&block);
            self.sections.push(Section {
                id: Default::default(),
                source: Some(self.source.to_string()),
                title: None,
                attributes: Attributes::default(),
                blocks: Vec::new(),
                position,
                span: None,
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
            // A section covers the source its blocks were read from,
            // which is everything from the heading that opened it to
            // the end of the last block under it.
            section.span = extent(&section.blocks);
        }
    }

    fn finish(mut self) -> (Vec<Section>, Vec<Warning>) {
        self.dangling();
        self.flush_section();
        (self.sections, self.warnings)
    }
}

/// Puts what an attribute line named on the block under it, the
/// line's own bytes included in what the block was read from.
fn annotate(block: &mut Block, line: Pending) {
    let (mine, span) = slots(block);
    merge(mine, line.attributes);
    if let Some(span) = span {
        span.start = span.start.min(line.read.span.start);
    }
}

/// Adds what a brace run named to what a block carries already.
/// Classes gather; an id written both above a heading and after it
/// is the line's.
fn merge(into: &mut Attributes, from: Attributes) {
    into.id = from.id.or_else(|| into.id.take());
    into.classes.extend(from.classes);
}

/// The names one block carries and the bytes it was read from.
fn slots(block: &mut Block) -> (&mut Attributes, &mut Option<SourceSpan>) {
    match block {
        Block::Heading {
            attributes, span, ..
        }
        | Block::Paragraph {
            attributes, span, ..
        }
        | Block::Blockquote {
            attributes, span, ..
        }
        | Block::ThematicBreak {
            attributes, span, ..
        }
        | Block::PageBreak {
            attributes, span, ..
        }
        | Block::ColumnBreak {
            attributes, span, ..
        }
        | Block::Image {
            attributes, span, ..
        }
        | Block::List {
            attributes, span, ..
        }
        | Block::Table {
            attributes, span, ..
        } => (attributes, span),
    }
}

/// The inside of a brace run, when the whole of an inline sequence
/// is one.
fn brace_text(inlines: &[Inline]) -> Option<&str> {
    let [Inline::Text { value, .. }] = inlines else {
        return None;
    };
    value.trim().strip_prefix('{')?.strip_suffix('}')
}

/// A brace run written at the head of one text run: what it names,
/// the text after the run, and where that text was read from.
fn brace_head(inline: &Inline) -> Option<(&str, &str, Option<SourceSpan>)> {
    let Inline::Text { value, span, .. } = inline else {
        return None;
    };
    let inside = value.strip_prefix('{')?;
    let close = inside.find('}')?;
    let rest = &inside[close + 1..];
    let taken = (value.len() - rest.len()) as u32;
    let moved = span.map(|span| SourceSpan {
        start: (span.start + taken).min(span.end),
        end: span.end,
    });
    Some((&inside[..close], rest, moved))
}

/// Whether one inline is a bracket the parser wrote on its own,
/// which is how a bracketed run with no destination arrives.
fn bracket(inline: &Inline, which: char) -> bool {
    matches!(inline, Inline::Text { value, .. } if value.len() == 1 && value.starts_with(which))
}

/// The classes and id a brace run holds: `.class` and `#id`, in any
/// order, at most one id. Nothing for a run holding anything else,
/// which is a run the vocabulary has no room for.
fn named(inside: &str) -> Option<Attributes> {
    let mut read = Attributes::default();
    for word in inside.split_whitespace() {
        // A second id is not a run the vocabulary can hold: an
        // element answers to one name.
        match word.split_at_checked(1)? {
            (".", class) => read.classes.push(identifier(class)?),
            ("#", id) if read.id.is_none() => read.id = Some(identifier(id)?),
            _ => return None,
        }
    }
    Some(read)
}

/// A name a selector can reach the node back by: a CSS identifier,
/// which is letters, digits, `-` and `_`, and does not open with a
/// digit.
fn identifier(word: &str) -> Option<String> {
    let opens = word.chars().next()?;
    let plain = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    (!opens.is_ascii_digit() && word.chars().all(plain)).then(|| word.to_string())
}

/// The alignment a delimiter row wrote on one column, if it wrote one.
fn alignment(written: &pulldown_cmark::Alignment) -> Option<Alignment> {
    match written {
        pulldown_cmark::Alignment::None => None,
        pulldown_cmark::Alignment::Left => Some(Alignment::Left),
        pulldown_cmark::Alignment::Center => Some(Alignment::Center),
        pulldown_cmark::Alignment::Right => Some(Alignment::Right),
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
    use crate::{Dialect, to_sections};
    use fleuron::content::{block_attributes, inline_span};

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

    /// An image written on its own line displaces no prose, and says
    /// nothing. One written among prose is set after the paragraph it
    /// was written in, and says so.
    #[test]
    fn only_an_image_among_prose_reports_the_move() {
        let (alone, quiet) = to_sections("![a plate](plate.jpg)\n", "test.md", &Options::default());
        assert!(quiet.is_empty(), "{quiet:?}");
        assert!(matches!(
            alone[0].blocks.as_slice(),
            [Block::Image { url, alt, .. }] if url == "plate.jpg" && alt == "a plate",
        ));

        let (among, loud) = to_sections(
            "Before ![a plate](plate.jpg) after.\n",
            "test.md",
            &Options::default(),
        );
        assert_eq!(loud.len(), 1, "{loud:?}");
        assert!(loud[0].message.contains("Inline images become blocks"));
        assert!(matches!(
            among[0].blocks.as_slice(),
            [Block::Paragraph { .. }, Block::Image { .. }],
        ));
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
        let (sections, _) = to_sections("# One\n\nA.\n\n# Two\n\nB.\n", "chapter-01.md", &whole());
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].blocks.len(), 4);
        assert_eq!(sections[0].position, Some(SourcePos { line: 1, column: 1 }));
        assert_eq!(sections[0].title, None);
    }

    /// A chapter file's frontmatter is the chapter's, so its `title:`
    /// names the section rather than the book the file is one of.
    #[test]
    fn a_whole_source_takes_its_title_from_its_frontmatter() {
        let markdown =
            "---\ntitle: The Ambassador\nstatus: draft\n---\n\nHe arrived on a Tuesday.\n";
        let (sections, _) = to_sections(markdown, "ch01.md", &whole());
        assert_eq!(sections[0].title.as_deref(), Some("The Ambassador"));

        // Cut at headings, the headings do the naming instead.
        let (sections, _) = to_sections(markdown, "ch01.md", &Options::default());
        assert_eq!(sections[0].title, None);
    }

    /// A chapter file's frontmatter names the section a sheet reaches
    /// with `section.front` and `section#preface`.
    #[test]
    fn a_whole_source_takes_its_class_and_id_from_its_frontmatter() {
        let markdown = "---\nclass: front roman\nid: preface\n---\n\nTo the reader.\n";
        let (sections, _) = to_sections(markdown, "preface.md", &whole());
        assert_eq!(
            sections[0].attributes,
            Attributes {
                id: Some("preface".into()),
                classes: vec!["front".into(), "roman".into()],
            }
        );

        let (sections, _) = to_sections(markdown, "preface.md", &Options::default());
        assert!(sections[0].attributes.is_empty());
    }

    fn whole() -> Options {
        Options {
            sections: Sections::Whole,
            ..Options::default()
        }
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

    /// Acceptance: a soft break is still a space.
    #[test]
    fn wrapped_lines_join_with_a_space() {
        let sections = read("# C\n\none\ntwo\n");
        assert_eq!(text_of(&sections[0].blocks[1]), "one two");
        let Block::Paragraph { inlines, .. } = &sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        assert!(
            !inlines.iter().any(|i| matches!(i, Inline::Break { .. })),
            "{inlines:?}",
        );
    }

    /// The inlines of a block that holds a hard break: the text
    /// before it, the break, and the text after it.
    fn broken(block: &Block) -> (&str, &str) {
        let (Block::Paragraph { inlines, .. } | Block::Heading { inlines, .. }) = block else {
            panic!("expected text, got {block:?}");
        };
        match inlines.as_slice() {
            [
                Inline::Text { value: before, .. },
                Inline::Break { .. },
                Inline::Text { value: after, .. },
            ] => (before, after),
            other => panic!("expected text, a break and text, got {other:?}"),
        }
    }

    /// Acceptance: two spaces at the end of a line break the line
    /// and stay in the paragraph.
    #[test]
    fn two_spaces_at_the_end_of_a_line_break_it_in_the_paragraph() {
        let markdown = "# C\n\nShall I compare thee  \nto a summer's day?\n";
        let sections = read(markdown);
        assert_eq!(sections[0].blocks.len(), 2, "{:?}", sections[0].blocks);
        assert_eq!(
            broken(&sections[0].blocks[1]),
            ("Shall I compare thee", "to a summer's day?"),
        );
        assert_eq!(
            text_of(&sections[0].blocks[1]),
            "Shall I compare thee\nto a summer's day?"
        );
    }

    /// Acceptance: a backslash at the end of a line does the same,
    /// and the break spans the backslash and the newline it was
    /// read from.
    #[test]
    fn a_backslash_at_the_end_of_a_line_breaks_it_in_the_paragraph() {
        let markdown = "# C\n\nShall I compare thee\\\nto a summer's day?\n";
        let sections = read(markdown);
        assert_eq!(sections[0].blocks.len(), 2, "{:?}", sections[0].blocks);
        assert_eq!(
            broken(&sections[0].blocks[1]),
            ("Shall I compare thee", "to a summer's day?"),
        );
        let Block::Paragraph { inlines, .. } = &sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        let span = inline_span(&inlines[1]).expect("the break was read from somewhere");
        assert_eq!(&markdown[span.start as usize..span.end as usize], "\\\n");
    }

    /// Acceptance: a backslash at the end of a line in a setext
    /// heading breaks the heading and stays in it.
    #[test]
    fn a_backslash_in_a_setext_heading_breaks_the_heading() {
        let markdown = "Chapter One\\\nThe Voyage to Lilliput\n======================\n\nProse.\n";
        let sections = read(markdown);
        let Block::Heading { level, .. } = &sections[0].blocks[0] else {
            panic!("expected a heading, got {:?}", sections[0].blocks);
        };
        assert_eq!(*level, HeadingLevel::H1);
        assert_eq!(sections[0].blocks.len(), 2, "{:?}", sections[0].blocks);
        assert_eq!(
            broken(&sections[0].blocks[0]),
            ("Chapter One", "The Voyage to Lilliput"),
        );
    }

    #[test]
    fn blockquotes_keep_their_blocks() {
        let sections = read("# C\n\n> Quoted.\n>\n> Still quoted.\n");
        let Block::Blockquote { blocks, .. } = &sections[0].blocks[1] else {
            panic!("expected a blockquote");
        };
        assert_eq!(blocks.len(), 2);
    }

    /// Acceptance: `\pagebreak` alone on a line is a page break, and
    /// `\columnbreak` a column break, each read from its own line.
    #[test]
    fn a_break_line_is_a_break() {
        let markdown = "# C\n\nBefore.\n\n\\pagebreak\n\nBetween.\n\n\\columnbreak\n\nAfter.\n";
        let (sections, warnings) = to_sections(markdown, "test.md", &Options::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        let blocks = &sections[0].blocks;
        assert!(matches!(blocks[2], Block::PageBreak { .. }), "{blocks:?}");
        assert!(matches!(blocks[4], Block::ColumnBreak { .. }), "{blocks:?}");
        assert_eq!(blocks.len(), 6, "{blocks:?}");
        let span = block_span(&blocks[2]).expect("the break was read from somewhere");
        assert_eq!(
            markdown[span.start as usize..span.end as usize].trim_end(),
            "\\pagebreak"
        );
    }

    /// Acceptance: `\pagebreak` among other prose in a paragraph stays
    /// prose and does not warn.
    #[test]
    fn a_break_among_prose_stays_prose() {
        let markdown = "# C\n\nTurn the page \\pagebreak here.\n\n\\pagebreak now\n";
        let (sections, warnings) = to_sections(markdown, "test.md", &Options::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        let blocks = &sections[0].blocks;
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert_eq!(text_of(&blocks[1]), "Turn the page \\pagebreak here.");
        assert_eq!(text_of(&blocks[2]), "\\pagebreak now");
    }

    /// Acceptance: an attribute line above a break names it.
    #[test]
    fn an_attribute_line_names_the_break_under_it() {
        let sections = read("# C\n\nA.\n\n{.soft #turn}\n\n\\pagebreak\n\nB.\n");
        let block = &sections[0].blocks[2];
        assert!(
            matches!(block, Block::PageBreak { .. }),
            "{:?}",
            sections[0].blocks
        );
        assert_eq!(
            block_attributes(block),
            &Attributes {
                id: Some("turn".into()),
                classes: vec!["soft".into()],
            }
        );
    }

    /// A break written in a blockquote or a list item is a block of
    /// that quote or that item.
    #[test]
    fn a_break_in_a_quote_or_an_item_is_a_break_there() {
        let sections = read("# C\n\n> A.\n>\n> \\pagebreak\n\n- one\n- \\columnbreak\n");
        let Block::Blockquote { blocks, .. } = &sections[0].blocks[1] else {
            panic!("expected a blockquote, got {:?}", sections[0].blocks);
        };
        assert!(
            matches!(
                blocks.as_slice(),
                [Block::Paragraph { .. }, Block::PageBreak { .. }]
            ),
            "{blocks:?}"
        );
        let Block::List { items, .. } = &sections[0].blocks[2] else {
            panic!("expected a list, got {:?}", sections[0].blocks);
        };
        assert!(
            matches!(items[1].blocks.as_slice(), [Block::ColumnBreak { .. }]),
            "{items:?}"
        );
    }

    /// Acceptance: under `Dialect::common_mark()`, `\pagebreak` is
    /// prose.
    #[test]
    fn under_common_mark_a_break_line_is_prose() {
        let options = Options {
            dialect: Dialect::common_mark(),
            ..Options::default()
        };
        let (sections, warnings) = to_sections("# C\n\n\\pagebreak\n", "test.md", &options);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(text_of(&sections[0].blocks[1]), "\\pagebreak");
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

    /// The construct a manuscript most often reaches for that the
    /// vocabulary has no room for. It says where it was written, and
    /// it leaves its prose behind.
    #[test]
    fn a_code_block_warns_and_keeps_its_prose() {
        let markdown = "# C\n\n```\ncode line\n```\n";
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
            [(
                "Code blocks are not supported. Falling back to a plain paragraph.",
                "test.md:3:1",
            )],
        );
        let prose: Vec<String> = sections[0].blocks[1..].iter().map(text_of).collect();
        assert_eq!(prose, ["code line\n"]);
    }

    /// The text of each item of a list, the lists nested in it left
    /// out.
    fn items_of(block: &Block) -> Vec<String> {
        let Block::List { items, .. } = block else {
            panic!("expected a list, got {block:?}");
        };
        items
            .iter()
            .map(|item| {
                item.blocks
                    .iter()
                    .filter(|block| !matches!(block, Block::List { .. }))
                    .map(text_of)
                    .collect()
            })
            .collect()
    }

    /// Acceptance: a list is a list. Its items and their prose reach
    /// the tree, and nothing warns.
    #[test]
    fn a_list_reads_into_items_and_warns_about_nothing() {
        let (sections, warnings) = to_sections(
            "# C\n\n- one\n- two *stressed*\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let list = &sections[0].blocks[1];
        let Block::List {
            ordered,
            start,
            tight,
            items,
            ..
        } = list
        else {
            panic!("expected a list, got {list:?}");
        };
        assert_eq!((*ordered, *start, *tight), (false, 1, true));
        assert_eq!(items_of(list), ["one", "two stressed"]);
        let Block::Paragraph { inlines, .. } = &items[1].blocks[0] else {
            panic!("an item holds a paragraph");
        };
        assert!(matches!(inlines[1], Inline::Emphasis { .. }), "{inlines:?}");
    }

    /// Acceptance: an ordered list counts from the number written on
    /// its first item.
    #[test]
    fn an_ordered_list_counts_from_the_number_on_its_first_item() {
        let sections = read("7. seven\n8. eight\n9. nine\n");
        let list = &sections[0].blocks[0];
        let Block::List { ordered, start, .. } = list else {
            panic!("expected a list, got {list:?}");
        };
        assert_eq!((*ordered, *start), (true, 7));
        assert_eq!(items_of(list), ["seven", "eight", "nine"]);
    }

    /// A blank line between two items makes the list loose, and an
    /// item of a loose list holds as many paragraphs as were written
    /// in it.
    #[test]
    fn a_blank_line_between_items_makes_the_list_loose() {
        let tight = |block: &Block| matches!(block, Block::List { tight: true, .. });
        let phrases = read("- one\n- two\n");
        assert!(tight(&phrases[0].blocks[0]));
        let paragraphs = read("- one\n\n- two\n");
        assert!(!tight(&paragraphs[0].blocks[0]));
        assert_eq!(items_of(&paragraphs[0].blocks[0]), ["one", "two"]);

        let long = read("- one\n\n  more\n- two\n");
        let Block::List { items, tight, .. } = &long[0].blocks[0] else {
            panic!("expected a list");
        };
        assert!(!tight);
        assert_eq!(items[0].blocks.len(), 2, "{:?}", items[0].blocks);
    }

    /// A list indented under an item is a block of that item, after
    /// the item's own text.
    #[test]
    fn a_list_indented_under_an_item_nests_inside_it() {
        let sections = read("- outer\n  - inner one\n  - inner two\n- after\n");
        assert_eq!(sections[0].blocks.len(), 1, "{:?}", sections[0].blocks);
        let list = &sections[0].blocks[0];
        assert_eq!(items_of(list), ["outer", "after"]);
        let Block::List { items, tight, .. } = list else {
            panic!("expected a list");
        };
        assert!(tight);
        assert!(
            matches!(
                items[0].blocks.as_slice(),
                [Block::Paragraph { .. }, Block::List { .. }]
            ),
            "{:?}",
            items[0].blocks,
        );
        assert_eq!(items_of(&items[0].blocks[1]), ["inner one", "inner two"]);
    }

    /// The line above a list names the list. The first item takes no
    /// name from it.
    #[test]
    fn an_attribute_line_names_the_list_and_not_its_first_item() {
        let (sections, warnings) = to_sections(
            "# C\n\n{.steps}\n\n1. one\n2. two\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let Block::List {
            items, attributes, ..
        } = &sections[0].blocks[1]
        else {
            panic!("expected a list, got {:?}", sections[0].blocks);
        };
        assert_eq!(attributes.classes, ["steps"]);
        assert!(items[0].attributes.is_empty());
        assert!(block_attributes(&items[0].blocks[0]).is_empty());
    }

    /// The paragraph of a tight item spans the words written in it,
    /// and the item spans its marker as well.
    #[test]
    fn the_text_of_a_tight_item_spans_its_words_and_the_item_its_marker() {
        let markdown = "- one\n- two\n";
        let sections = read(markdown);
        let covered = |span: Option<SourceSpan>| {
            let span = span.expect("a parsed node was read from somewhere");
            &markdown[span.start as usize..span.end as usize]
        };
        let Block::List { items, span, .. } = &sections[0].blocks[0] else {
            panic!("expected a list");
        };
        assert_eq!(covered(*span), markdown);
        assert_eq!(covered(items[1].span), "- two\n");
        assert_eq!(covered(block_span(&items[1].blocks[0])), "two");
    }

    /// An image written in an item stays in the item.
    #[test]
    fn an_image_in_an_item_stays_in_the_item() {
        let (sections, warnings) = to_sections(
            "- ![a map](map.png)\n- two\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(sections[0].blocks.len(), 1);
        let Block::List { items, .. } = &sections[0].blocks[0] else {
            panic!("expected a list, got {:?}", sections[0].blocks);
        };
        assert!(
            matches!(items[0].blocks.as_slice(), [Block::Image { url, .. }] if url == "map.png"),
            "{:?}",
            items[0].blocks,
        );
    }

    /// The text of every cell of one row, from the leading edge.
    fn cells_of(row: &Row) -> Vec<String> {
        row.cells
            .iter()
            .map(|cell| cell.blocks.iter().map(text_of).collect())
            .collect()
    }

    /// Acceptance: a table is a table. Its header row, its body rows,
    /// the prose of each cell and the alignment the delimiter row
    /// wrote on each column all reach the tree, and nothing warns.
    #[test]
    fn a_table_reads_into_rows_of_cells_and_warns_about_nothing() {
        let markdown = "\
# C

| Pocket | Found | Kept |
|:---|---:|:---:|
| The right fob | A *watch* | no |
| The girdle | A scimitar |
";
        let (sections, warnings) = to_sections(markdown, "test.md", &Options::default());
        assert!(warnings.is_empty(), "{warnings:?}");
        let Block::Table { head, body, .. } = &sections[0].blocks[1] else {
            panic!("expected a table, got {:?}", sections[0].blocks[1]);
        };
        assert_eq!(head.len(), 1);
        assert_eq!(cells_of(&head[0]), ["Pocket", "Found", "Kept"]);
        assert_eq!(body.len(), 2);
        assert_eq!(cells_of(&body[0]), ["The right fob", "A watch", "no"]);
        assert_eq!(cells_of(&body[1])[..2], ["The girdle", "A scimitar"]);
        assert!(body[1].cells[2..].iter().all(|cell| cell.blocks.is_empty()));

        let written = [
            Some(Alignment::Left),
            Some(Alignment::Right),
            Some(Alignment::Center),
        ];
        for row in head.iter().chain(body) {
            let aligns: Vec<Option<Alignment>> = row.cells.iter().map(|cell| cell.align).collect();
            assert_eq!(aligns, written[..aligns.len()]);
        }
        let Block::Paragraph { inlines, .. } = &body[0].cells[1].blocks[0] else {
            panic!("a cell holds a paragraph");
        };
        assert!(matches!(inlines[1], Inline::Emphasis { .. }), "{inlines:?}");
    }

    /// A column the delimiter row wrote no colon on has no alignment.
    #[test]
    fn a_column_written_without_a_colon_has_no_alignment() {
        let sections = read("| a | b |\n|---|--:|\n| c | d |\n");
        let Block::Table { body, .. } = &sections[0].blocks[0] else {
            panic!("expected a table");
        };
        let aligns: Vec<Option<Alignment>> = body[0].cells.iter().map(|cell| cell.align).collect();
        assert_eq!(aligns, [None, Some(Alignment::Right)]);
    }

    /// Under CommonMark a table is the prose it was written as, and
    /// nothing warns, because nothing was lost.
    #[test]
    fn common_mark_reads_a_table_as_prose() {
        let plain = Options {
            dialect: Dialect::common_mark(),
            ..Options::default()
        };
        let (sections, warnings) =
            to_sections("| a | b |\n|---|---|\n| c | d |\n", "test.md", &plain);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            matches!(sections[0].blocks.as_slice(), [Block::Paragraph { .. }]),
            "{:?}",
            sections[0].blocks,
        );
    }

    /// The line above a table names the table, as it names any
    /// block.
    #[test]
    fn an_attribute_line_names_the_table_under_it() {
        let sections = read("# C\n\n{.inventory}\n\n| a | b |\n|---|---|\n| c | d |\n");
        assert!(matches!(sections[0].blocks[1], Block::Table { .. }));
        assert_eq!(
            block_attributes(&sections[0].blocks[1]).classes,
            ["inventory"]
        );
    }

    /// An image written in a cell is a block of that cell, and not of
    /// the section around the table.
    #[test]
    fn an_image_in_a_cell_stays_in_the_cell() {
        let (sections, warnings) = to_sections(
            "| Plate |\n|---|\n| ![a map](map.png) |\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(sections[0].blocks.len(), 1);
        let Block::Table { body, .. } = &sections[0].blocks[0] else {
            panic!("expected a table");
        };
        assert!(
            matches!(
                body[0].cells[0].blocks.as_slice(),
                [Block::Image { url, .. }] if url == "map.png"
            ),
            "{:?}",
            body[0].cells[0].blocks,
        );
    }

    /// A cell answers a cursor the way a paragraph does, with the run
    /// the byte was typed into.
    #[test]
    fn a_cell_answers_a_cursor_as_prose_does() {
        let markdown = "| Pocket | Found |\n|---|---|\n| The right fob | A watch |\n";
        let (sections, _) = to_sections(markdown, "test.md", &Options::default());
        let book = crate::assemble(Default::default(), sections);
        for written in ["Pocket", "fob", "watch"] {
            let byte = markdown.find(written).expect("the fixture holds it") as u32;
            let node = book
                .node_at("test.md", byte)
                .unwrap_or_else(|| panic!("nothing was read from {written:?}"));
            let (_, span) = book.source_of(node).expect("and it says where");
            assert!(
                markdown[span.start as usize..span.end as usize].contains(written),
                "{written:?} answered with {:?}",
                &markdown[span.start as usize..span.end as usize],
            );
        }
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

    /// A node's span is the bytes of the source it was read from,
    /// markup and all, and the nodes under it fall inside it.
    #[test]
    fn a_node_spans_the_source_it_was_read_from() {
        let markdown = "# One\n\nPlain *stressed* plain.\n";
        let sections = read(markdown);
        let covered = |span: Option<SourceSpan>| {
            let span = span.expect("a parsed node was read from somewhere");
            &markdown[span.start as usize..span.end as usize]
        };

        let Block::Heading { span, .. } = &sections[0].blocks[0] else {
            panic!("expected a heading");
        };
        assert_eq!(covered(*span), "# One\n");

        let Block::Paragraph { span, inlines, .. } = &sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        assert_eq!(covered(*span), "Plain *stressed* plain.\n");
        assert_eq!(covered(inline_span(&inlines[0])), "Plain ");
        let Inline::Emphasis { span, children, .. } = &inlines[1] else {
            panic!("expected an emphasis");
        };
        assert_eq!(covered(*span), "*stressed*");
        assert_eq!(covered(inline_span(&children[0])), "stressed");

        // The section runs from the heading that opened it to the end
        // of the last block under it.
        assert_eq!(covered(sections[0].span), markdown);
    }

    /// A wrapped line is a space in the tree and a newline in the
    /// file, and the space says which newline it was read from.
    #[test]
    fn a_wrapped_line_spans_the_break_it_was_read_from() {
        let markdown = "# C\n\none\ntwo\n";
        let sections = read(markdown);
        let Block::Paragraph { inlines, .. } = &sections[0].blocks[1] else {
            panic!("expected a paragraph");
        };
        let spaces: Vec<Option<SourceSpan>> = inlines
            .iter()
            .filter(|inline| matches!(inline, Inline::Text { value, .. } if value == " "))
            .map(inline_span)
            .collect();
        assert_eq!(spaces, [Some(SourceSpan { start: 8, end: 9 })]);
        assert_eq!(&markdown[8..9], "\n");
    }

    /// Acceptance: a heading, a quotation and a list item answer a
    /// cursor the way a paragraph does, with the run the byte was
    /// typed into.
    #[test]
    fn a_heading_a_quotation_and_a_list_item_answer_as_prose_does() {
        let markdown = "\
# The Heading

Ordinary prose.

> Quoted prose.

- An item
";
        let (sections, _) = to_sections(
            markdown,
            "test.md",
            &Options {
                dialect: Dialect::gfm(),
                ..Options::default()
            },
        );
        let book = crate::assemble(Default::default(), sections);

        for written in ["Heading", "prose.", "Quoted", "item"] {
            let byte = markdown.find(written).expect("the fixture holds it") as u32;
            let node = book
                .node_at("test.md", byte)
                .unwrap_or_else(|| panic!("nothing was read from {written:?}"));
            let (source, span) = book.source_of(node).expect("and it says where");
            assert_eq!(source, "test.md");
            assert!(
                markdown[span.start as usize..span.end as usize].contains(written),
                "{written:?} answered with {:?}",
                &markdown[span.start as usize..span.end as usize],
            );
        }
    }

    /// An image written among prose is set after the paragraph it was
    /// written in, and its span stays where it was written, so the
    /// bytes of the image answer with the image.
    #[test]
    fn a_displaced_image_answers_for_the_bytes_it_was_written_at() {
        let markdown = "# C\n\nProse ![a map](map.png) more.\n";
        let (sections, _) = to_sections(markdown, "test.md", &Options::default());
        let book = crate::assemble(Default::default(), sections);

        let at = markdown.find("map.png").expect("the fixture holds it") as u32;
        let node = book
            .node_at("test.md", at)
            .expect("the image was read there");
        let Block::Image { id, .. } = &book.sections[0].blocks[2] else {
            panic!("expected an image block");
        };
        assert_eq!(node, *id);
    }

    /// The line above a quote names the quote. The blocks inside
    /// the quote take their own names, and the first of them does
    /// not take the line's.
    #[test]
    fn an_attribute_line_names_the_blockquote_and_not_its_first_paragraph() {
        let sections = read("# C\n\n{.epigraph}\n> Man is the only animal that blushes.\n");
        let Block::Blockquote {
            blocks, attributes, ..
        } = &sections[0].blocks[1]
        else {
            panic!("expected a blockquote");
        };
        assert_eq!(attributes.classes, ["epigraph"]);
        assert!(block_attributes(&blocks[0]).is_empty(), "{blocks:?}");
    }

    /// `---` under a line of text is a setext heading in CommonMark,
    /// so the break an author asked for has to be read back out of
    /// the heading it was parsed into.
    #[test]
    fn an_attribute_line_reaches_the_thematic_break_under_it() {
        for markdown in ["# C\n\n{.ornament}\n---\n", "# C\n\n{.ornament}\n\n---\n"] {
            let sections = read(markdown);
            let Block::ThematicBreak { attributes, .. } = &sections[0].blocks[1] else {
                panic!("expected a scene break for {markdown:?}");
            };
            assert_eq!(attributes.classes, ["ornament"], "{markdown:?}");
        }
    }

    /// The two ways of writing a heading's classes are one class.
    #[test]
    fn a_heading_takes_the_same_class_written_over_it_or_after_it() {
        let over = read("{.opening}\n# Chapter One\n");
        let after = read("# Chapter One {.opening}\n");
        assert_eq!(
            block_attributes(&over[0].blocks[0]),
            block_attributes(&after[0].blocks[0]),
        );
        assert_eq!(block_attributes(&over[0].blocks[0]).classes, ["opening"]);
    }

    /// An image alone on its line takes the run written after it,
    /// which is the only trailing form that is not a heading's.
    #[test]
    fn an_image_alone_on_its_line_takes_the_run_after_it() {
        let (sections, warnings) = to_sections(
            "# C\n\n![a map](plate.jpg){.map}\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let Block::Image { attributes, .. } = &sections[0].blocks[1] else {
            panic!("expected an image");
        };
        assert_eq!(attributes.classes, ["map"]);
    }

    /// A line that names nothing is the prose it was read as, and
    /// warns: at the end of a section, and where a second line takes
    /// its place.
    #[test]
    fn an_attribute_line_over_nothing_stays_prose_and_warns() {
        let (sections, warnings) = to_sections(
            "# C\n\n{.first}\n\n{.second}\n\nProse.\n\n{.last}\n",
            "test.md",
            &Options::default(),
        );
        let text: Vec<String> = sections[0].blocks[1..].iter().map(text_of).collect();
        assert_eq!(text, ["{.first}", "Prose.", "{.last}"]);
        assert_eq!(block_attributes(&sections[0].blocks[2]).classes, ["second"]);
        let at: Vec<&str> = warnings
            .iter()
            .map(|warning| warning.origin.as_deref().unwrap_or_default())
            .collect();
        assert_eq!(at, ["test.md:3:1", "test.md:9:1"], "{warnings:?}");
    }

    /// One manuscript with every construct the vocabulary has no room
    /// for. Each warning is a sentence, and names the construct and
    /// what it falls back to.
    #[test]
    fn every_frontend_warning_reads_as_a_sentence() {
        let markdown = "# C\n\n\
             ```\ncode\n```\n\n\
             | a | b |\n|---|---|\n| c | d |\n\n\
             ~~struck~~ and $x$ and <b>bold</b>\n\n\
             <div>block</div>\n\n\
             A note[^1] and a run ![a map](p.jpg) among prose.\n\n\
             [^1]: The note.\n\n\
             {key=value}\n";
        let (_, warnings) = to_sections(markdown, "test.md", &Options::default());
        assert!(warnings.len() >= 7, "{warnings:?}");
        for warning in &warnings {
            let message = &warning.message;
            assert!(
                message.starts_with(|opens: char| opens.is_uppercase()),
                "{message}",
            );
            assert!(message.ends_with('.'), "{message}");
            assert!(!message.contains(';'), "{message}");
            assert!(
                message.to_lowercase().contains("supported") || message.contains("become"),
                "{message}",
            );
        }
    }

    /// A brace run the vocabulary has no room for is prose, the same
    /// as every other construct it has no room for.
    #[test]
    fn a_brace_run_that_is_not_classes_and_an_id_stays_prose_and_warns() {
        for run in ["{key=value}", "{#one #two}", "{.9lives}"] {
            let (sections, warnings) = to_sections(
                &format!("# C\n\n{run}\n\nProse.\n"),
                "test.md",
                &Options::default(),
            );
            assert_eq!(text_of(&sections[0].blocks[1]), run);
            assert!(block_attributes(&sections[0].blocks[2]).is_empty());
            assert_eq!(warnings.len(), 1, "{run}: {warnings:?}");
            assert!(
                warnings[0].message.contains("Unsupported attribute"),
                "{warnings:?}"
            );
        }
    }

    /// The inlines of one block, whatever kind of block it is.
    fn inlines_of(block: &Block) -> &[Inline] {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => inlines,
            other => panic!("expected text, got {other:?}"),
        }
    }

    /// The classes of one span, and the text inside it.
    fn spans(inlines: &[Inline]) -> Vec<(Vec<String>, String)> {
        inlines
            .iter()
            .filter_map(|inline| match inline {
                Inline::Span { attributes, .. } => Some((
                    attributes.classes.clone(),
                    inline_text(std::slice::from_ref(inline)),
                )),
                _ => None,
            })
            .collect()
    }

    /// Acceptance: a bracketed run followed by a brace run is a span
    /// the sheet names, and the prose around it is untouched.
    #[test]
    fn a_bracketed_run_before_a_brace_run_is_a_span() {
        let (sections, warnings) = to_sections(
            "# C\n\nA [some words]{.number #one} b.\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let inlines = inlines_of(&sections[0].blocks[1]);
        assert_eq!(inline_text(inlines), "A some words b.");
        let [
            Inline::Text { .. },
            Inline::Span { attributes, .. },
            Inline::Text { value, .. },
        ] = inlines
        else {
            panic!("expected a span among prose, got {inlines:?}");
        };
        assert_eq!(attributes.classes, ["number"]);
        assert_eq!(attributes.id.as_deref(), Some("one"));
        assert_eq!(value, " b.");
    }

    /// Acceptance: two runs of one heading, each named by the sheet,
    /// inside the one heading. The run at the end of the line is the
    /// span's rather than the heading's.
    #[test]
    fn a_heading_holds_two_named_runs() {
        let (sections, warnings) = to_sections(
            "# [Chapter One]{.number} [The Road]{.title}\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let heading = &sections[0].blocks[0];
        assert!(block_attributes(heading).is_empty(), "{heading:?}");
        assert_eq!(text_of(heading), "Chapter One The Road");
        assert_eq!(
            spans(inlines_of(heading)),
            [
                (vec!["number".to_string()], "Chapter One".to_string()),
                (vec!["title".to_string()], "The Road".to_string()),
            ]
        );
    }

    /// A heading still takes the run written after its own text.
    #[test]
    fn a_heading_keeps_a_trailing_run_of_its_own() {
        let sections = read("# [Chapter One]{.number} The Road {#ch1 .grand}\n");
        let heading = &sections[0].blocks[0];
        assert_eq!(
            block_attributes(heading),
            &Attributes {
                id: Some("ch1".into()),
                classes: vec!["grand".into()],
            }
        );
        assert_eq!(text_of(heading), "Chapter One The Road");
        assert_eq!(
            spans(inlines_of(heading)),
            [(vec!["number".to_string()], "Chapter One".to_string())]
        );
    }

    /// A span holds the markup written inside it, and a span written
    /// inside another is its child.
    #[test]
    fn a_span_holds_the_markup_inside_it() {
        let sections = read("# C\n\n[a *b* [c]{.inner}]{.outer}\n");
        let inlines = inlines_of(&sections[0].blocks[1]);
        let [
            Inline::Span {
                children,
                attributes,
                ..
            },
        ] = inlines
        else {
            panic!("expected one span, got {inlines:?}");
        };
        assert_eq!(attributes.classes, ["outer"]);
        assert_eq!(inline_text(children), "a b c");
        assert!(
            matches!(children[1], Inline::Emphasis { .. }),
            "{children:?}"
        );
        assert_eq!(
            spans(children),
            [(vec!["inner".to_string()], "c".to_string())]
        );
    }

    /// A span is read from the bytes it was written at, the brackets
    /// and the run included.
    #[test]
    fn a_span_is_read_from_the_bytes_it_was_written_at() {
        let markdown = "# C\n\nA [words]{.number} b.\n";
        let sections = read(markdown);
        let inlines = inlines_of(&sections[0].blocks[1]);
        let Inline::Span { position, span, .. } = &inlines[1] else {
            panic!("expected a span, got {inlines:?}");
        };
        let span = span.expect("a parsed span was read from somewhere");
        assert_eq!(
            &markdown[span.start as usize..span.end as usize],
            "[words]{.number}"
        );
        assert_eq!(*position, Some(SourcePos { line: 3, column: 3 }));
        let Inline::Text { span: after, .. } = &inlines[2] else {
            panic!("expected prose after the span, got {inlines:?}");
        };
        let after = after.expect("prose was read from somewhere");
        assert_eq!(&markdown[after.start as usize..after.end as usize], " b.");
    }

    /// Acceptance: the brace run has to follow the bracket directly.
    /// A gap between them is prose, and says nothing.
    #[test]
    fn a_gap_before_the_brace_run_is_prose() {
        let (sections, warnings) = to_sections(
            "# C\n\nA [words] {.number} b.\n",
            "test.md",
            &Options::default(),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        let inlines = inlines_of(&sections[0].blocks[1]);
        assert_eq!(inline_text(inlines), "A [words] {.number} b.");
        assert!(spans(inlines).is_empty(), "{inlines:?}");
    }

    /// Acceptance: a run the syntax cannot hold stays prose and
    /// warns, the same as one written on a line of its own.
    #[test]
    fn a_span_run_the_syntax_cannot_hold_stays_prose_and_warns() {
        for run in ["{key=value}", "{#one #two}", "{.9lives}"] {
            let (sections, warnings) = to_sections(
                &format!("# C\n\nA [words]{run} b.\n"),
                "test.md",
                &Options::default(),
            );
            let inlines = inlines_of(&sections[0].blocks[1]);
            assert_eq!(inline_text(inlines), format!("A [words]{run} b."));
            assert!(spans(inlines).is_empty(), "{run}: {inlines:?}");
            assert_eq!(warnings.len(), 1, "{run}: {warnings:?}");
            assert!(
                warnings[0].message.contains("Unsupported attribute"),
                "{warnings:?}"
            );
        }
    }

    /// Acceptance: under CommonMark the brackets and the braces are
    /// prose, and the tree holds no span.
    #[test]
    fn common_mark_reads_a_bracketed_run_as_prose() {
        let plain = Options {
            dialect: Dialect::common_mark(),
            ..Options::default()
        };
        let (sections, warnings) = to_sections(
            "# [Chapter One]{.number} [The Road]{.title}\n\nA [words]{.number} b.\n",
            "test.md",
            &plain,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            text_of(&sections[0].blocks[0]),
            "[Chapter One]{.number} [The Road]{.title}"
        );
        assert_eq!(text_of(&sections[0].blocks[1]), "A [words]{.number} b.");
        assert!(
            sections[0]
                .blocks
                .iter()
                .all(|block| spans(inlines_of(block)).is_empty()),
            "{:?}",
            sections[0].blocks,
        );
    }

    /// Under CommonMark the braces are four characters of prose, and
    /// the tree is the tree that dialect always read.
    #[test]
    fn common_mark_reads_a_brace_run_as_prose() {
        let markdown = "# C {.opening}\n\n{.epigraph}\n\n> Quoted.\n\n![a map](p.jpg){.map}\n";
        let plain = Options {
            dialect: Dialect::common_mark(),
            ..Options::default()
        };
        let (sections, warnings) = to_sections(markdown, "test.md", &plain);
        assert!(
            sections[0]
                .blocks
                .iter()
                .all(|block| block_attributes(block).is_empty()),
            "{:?}",
            sections[0].blocks,
        );
        assert_eq!(text_of(&sections[0].blocks[0]), "C {.opening}");
        assert_eq!(text_of(&sections[0].blocks[1]), "{.epigraph}");
        // The trailing run is prose too, which is the paragraph the
        // image is broken out of.
        assert_eq!(text_of(&sections[0].blocks[3]), "{.map}");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].message.contains("Inline images become blocks"));
    }

    #[test]
    fn an_inline_image_becomes_a_block_after_the_paragraph_that_held_it() {
        let (sections, warnings) = to_sections(
            "# C\n\nProse ![a map](map.png) more.\n",
            "test.md",
            &Options::default(),
        );
        assert_eq!(text_of(&sections[0].blocks[1]), "Prose  more.");
        let Block::Image { url, alt, .. } = &sections[0].blocks[2] else {
            panic!("expected an image block");
        };
        assert_eq!((url.as_str(), alt.as_str()), ("map.png", "a map"));
        assert_eq!(warnings.len(), 1);
    }
}
