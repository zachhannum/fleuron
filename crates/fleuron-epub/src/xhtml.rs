//! The content tree as XHTML: one document per section, with the
//! element names the sheets already select.
//!
//! The mapping follows the element tree the cascade matches against,
//! so a selector that reaches a node in the PDF reaches the same node
//! here. Two names have no element in HTML: the book is the `body`,
//! and a note is an `aside` after the section, with a numbered link
//! where the note was written.

use std::collections::{BTreeMap, BTreeSet};

use fleuron::Warning;
use fleuron::content::{
    Alignment, Anchors, Attributes, Block, Book, HeadingLevel, Inline, LinkTarget, ListItem,
    NodeId, Section, SourcePos, origin, rows, text,
};

use crate::media::{Kind, Refused, Resources};
use crate::xml;

/// The attribute a block element carries its node id in.
pub const NODE: &str = "data-node";

/// What a node is to the ids the plan gives out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Heading,
    /// A block that is an element in its document.
    Block,
    Note,
    Other,
}

/// Where each element that something links to is: the document it is
/// in, and the id it carries there.
#[derive(Debug, Default)]
pub struct Plan {
    /// Each section's document, in reading order.
    pub hrefs: Vec<String>,
    /// The document of each section.
    sections: BTreeMap<NodeId, usize>,
    /// The document and id of every element that carries an id.
    ids: BTreeMap<NodeId, (usize, String)>,
}

impl Plan {
    /// Names each section's document and gives an id to every node
    /// that has one written, every block, every note and every node a
    /// link reaches.
    pub fn of(book: &Book, anchors: &Anchors) -> Plan {
        let mut targets = BTreeSet::new();
        for section in &book.sections {
            let source = section.source.as_deref();
            each_block(&section.blocks, &mut |node| {
                if let Visit::Link(url) = node
                    && let LinkTarget::Node(target) = anchors.resolve(url, source)
                {
                    targets.insert(target);
                }
            });
        }

        let mut plan = Plan::default();
        for (doc, section) in book.sections.iter().enumerate() {
            plan.hrefs.push(format!("section-{:03}.xhtml", doc + 1));
            plan.sections.insert(section.id, doc);

            // Every id the document writes is taken before any is
            // made up, so a made-up one never takes a written one.
            let mut written: Vec<(NodeId, String)> = Vec::new();
            let mut wanted: Vec<(NodeId, Role)> = Vec::new();
            let mut take = |id: NodeId, attributes: &Attributes, role: Role| {
                if let Some(name) = attributes.id.as_deref() {
                    written.push((id, name.to_string()));
                } else if role != Role::Other || targets.contains(&id) {
                    wanted.push((id, role));
                }
            };
            take(section.id, &section.attributes, Role::Block);
            each_block(&section.blocks, &mut |node| {
                if let Visit::Node(id, attributes, role) = node {
                    take(id, attributes, role);
                }
            });

            let mut taken = BTreeSet::new();
            for (id, name) in written {
                if taken.insert(name.clone()) {
                    plan.ids.insert(id, (doc, name));
                }
            }
            let mut notes = 0;
            for (id, role) in wanted {
                let stem = match role {
                    Role::Note => {
                        notes += 1;
                        format!("note-{notes}")
                    }
                    Role::Heading | Role::Block | Role::Other => format!("n{}", id.get()),
                };
                let mut name = stem.clone();
                let mut again = 1;
                while !taken.insert(name.clone()) {
                    again += 1;
                    name = format!("{stem}-{again}");
                }
                plan.ids.insert(id, (doc, name));
            }
        }
        plan
    }

    /// The id a node carries in its document, if it carries one.
    fn id(&self, node: NodeId) -> Option<&str> {
        self.ids.get(&node).map(|(_, id)| id.as_str())
    }

    /// Where a link to `node` goes: its document, and its id there
    /// unless it is a section, which is the whole document.
    pub fn href(&self, node: NodeId) -> Option<String> {
        if let Some(doc) = self.sections.get(&node) {
            return Some(self.hrefs[*doc].clone());
        }
        let (doc, id) = self.ids.get(&node)?;
        Some(format!("{}#{id}", self.hrefs[*doc]))
    }
}

/// One node the walk over a section passes, in document order.
enum Visit<'a> {
    Node(NodeId, &'a Attributes, Role),
    Link(&'a str),
}

fn each_block<'a>(blocks: &'a [Block], f: &mut dyn FnMut(Visit<'a>)) {
    for block in blocks {
        match block {
            Block::Heading {
                id,
                inlines,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Heading));
                each_inline(inlines, f);
            }
            Block::Paragraph {
                id,
                inlines,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Block));
                each_inline(inlines, f);
            }
            Block::Blockquote {
                id,
                blocks,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Block));
                each_block(blocks, f);
            }
            Block::CodeBlock { id, attributes, .. }
            | Block::ThematicBreak { id, attributes, .. }
            | Block::Image { id, attributes, .. } => f(Visit::Node(*id, attributes, Role::Block)),
            // A break has no element, so it has an id only when a
            // link reaches it or the author wrote one.
            Block::PageBreak { id, attributes, .. } | Block::ColumnBreak { id, attributes, .. } => {
                f(Visit::Node(*id, attributes, Role::Other))
            }
            Block::List {
                id,
                items,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Block));
                for item in items {
                    f(Visit::Node(item.id, &item.attributes, Role::Block));
                    each_block(&item.blocks, f);
                }
            }
            Block::Table {
                id,
                head,
                body,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Block));
                for row in rows(head, body) {
                    f(Visit::Node(row.id, &row.attributes, Role::Block));
                    for cell in &row.cells {
                        f(Visit::Node(cell.id, &cell.attributes, Role::Block));
                        each_block(&cell.blocks, f);
                    }
                }
            }
        }
    }
}

fn each_inline<'a>(inlines: &'a [Inline], f: &mut dyn FnMut(Visit<'a>)) {
    for inline in inlines {
        match inline {
            // A run of text and a break are no elements, so a name on
            // one has nothing to go on.
            Inline::Text { .. } | Inline::Break { .. } => {}
            Inline::Code { id, attributes, .. } => f(Visit::Node(*id, attributes, Role::Other)),
            Inline::Link {
                id,
                url,
                children,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Other));
                f(Visit::Link(url));
                each_inline(children, f);
            }
            Inline::Emphasis {
                id,
                children,
                attributes,
                ..
            }
            | Inline::Strong {
                id,
                children,
                attributes,
                ..
            }
            | Inline::Strikethrough {
                id,
                children,
                attributes,
                ..
            }
            | Inline::Span {
                id,
                children,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Other));
                each_inline(children, f);
            }
            Inline::Note {
                id,
                blocks,
                attributes,
                ..
            } => {
                f(Visit::Node(*id, attributes, Role::Note));
                each_block(blocks, f);
            }
        }
    }
}

/// What writing one document needs from the rest of the book.
pub struct Context<'a> {
    pub plan: &'a Plan,
    pub anchors: &'a Anchors,
    /// The book's language, for the root element.
    pub language: Option<&'a str>,
    pub resources: &'a mut Resources,
    pub images: &'a dyn fleuron::images::ImageLoader,
    pub warnings: &'a mut Vec<Warning>,
}

/// One section as a document: its title, for the head, and the whole
/// file.
pub fn document(section: &Section, title: &str, cx: &mut Context<'_>) -> String {
    let mut writer = Writer {
        out: String::new(),
        source: section.source.as_deref(),
        notes: Vec::new(),
        numbers: BTreeMap::new(),
        cx,
    };
    writer.open("section", section.id, &section.attributes, &[]);
    writer.out.push('\n');
    writer.blocks(&section.blocks, false);
    writer.out.push_str("</section>\n");
    writer.notes();
    let body = writer.out;
    let mut out = String::new();
    head(&mut out, title, cx.language, Some("book.css"));
    out.push_str("<body>\n");
    out.push_str(&body);
    out.push_str("</body>\n</html>\n");
    out
}

/// The top of an XHTML document, up to the end of its head.
pub fn head(out: &mut String, title: &str, language: Option<&str>, sheet: Option<&str>) {
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n");
    out.push_str(
        "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"",
    );
    if let Some(language) = language {
        out.push_str(" xml:lang=\"");
        xml::attribute(out, language);
        out.push_str("\" lang=\"");
        xml::attribute(out, language);
        out.push('"');
    }
    out.push_str(">\n<head>\n<title>");
    xml::text(out, title);
    out.push_str("</title>\n");
    if let Some(sheet) = sheet {
        out.push_str("<link rel=\"stylesheet\" type=\"text/css\" href=\"");
        xml::attribute(out, sheet);
        out.push_str("\"/>\n");
    }
    out.push_str("</head>\n");
}

struct Writer<'s, 'c, 'a> {
    out: String,
    source: Option<&'s str>,
    /// The notes written so far whose own text is still to come, in
    /// the order their references were written.
    notes: Vec<&'s Inline>,
    /// The number each note's reference printed.
    numbers: BTreeMap<NodeId, usize>,
    cx: &'c mut Context<'a>,
}

impl<'s> Writer<'s, '_, '_> {
    /// Writes the opening tag of a block with the node's id, the node
    /// itself and its classes, and any other attributes after them.
    fn open(&mut self, tag: &str, node: NodeId, attributes: &Attributes, extra: &[(&str, &str)]) {
        self.tag(tag, node, attributes, extra, true);
    }

    /// The same for an inline, which does not carry its node.
    fn open_inline(
        &mut self,
        tag: &str,
        node: NodeId,
        attributes: &Attributes,
        extra: &[(&str, &str)],
    ) {
        self.tag(tag, node, attributes, extra, false);
    }

    fn tag(
        &mut self,
        tag: &str,
        node: NodeId,
        attributes: &Attributes,
        extra: &[(&str, &str)],
        block: bool,
    ) {
        self.out.push('<');
        self.out.push_str(tag);
        if let Some(id) = self.cx.plan.id(node) {
            self.attribute("id", id);
        }
        // The id can be the author's, or a made-up one that gave way
        // to the author's, so the node is written apart from it.
        if block {
            self.attribute(NODE, &node.get().to_string());
        }
        if !attributes.classes.is_empty() {
            self.attribute("class", &attributes.classes.join(" "));
        }
        for (name, value) in extra {
            self.attribute(name, value);
        }
        self.out.push('>');
    }

    fn attribute(&mut self, name: &str, value: &str) {
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        xml::attribute(&mut self.out, value);
        self.out.push('"');
    }

    fn close(&mut self, tag: &str) {
        self.out.push_str("</");
        self.out.push_str(tag);
        self.out.push('>');
    }

    fn warn(&mut self, message: String, position: Option<SourcePos>) {
        let at = origin(self.source, position);
        self.cx.warnings.push(Warning {
            message,
            origin: (!at.is_empty()).then_some(at),
        });
    }

    /// `bare` is an item of a tight list, whose paragraphs with no
    /// names of their own are its text rather than elements.
    fn blocks(&mut self, blocks: &'s [Block], bare: bool) {
        for block in blocks {
            let before = self.out.len();
            self.block(block, bare);
            if self.out.len() > before {
                self.out.push('\n');
            }
        }
    }

    fn block(&mut self, block: &'s Block, bare: bool) {
        match block {
            Block::Heading {
                id,
                level,
                inlines,
                attributes,
                ..
            } => {
                let tag = heading(*level);
                self.open(tag, *id, attributes, &[]);
                self.inlines(inlines);
                self.close(tag);
            }
            Block::Paragraph {
                inlines,
                attributes,
                ..
            } if bare && attributes.is_empty() => self.inlines(inlines),
            Block::Paragraph {
                id,
                inlines,
                attributes,
                ..
            } => {
                self.open("p", *id, attributes, &[]);
                self.inlines(inlines);
                self.close("p");
            }
            Block::Blockquote {
                id,
                blocks,
                attributes,
                ..
            } => {
                self.open("blockquote", *id, attributes, &[]);
                self.out.push('\n');
                self.blocks(blocks, false);
                self.close("blockquote");
            }
            Block::CodeBlock {
                id,
                text,
                attributes,
                ..
            } => {
                self.open("pre", *id, attributes, &[]);
                xml::text(&mut self.out, text);
                self.close("pre");
            }
            Block::ThematicBreak { id, attributes, .. } => {
                self.open("hr", *id, attributes, &[]);
                self.close("hr");
            }
            // A reading system makes its own pages and columns, so a
            // break between them has nothing to write.
            Block::PageBreak { .. } | Block::ColumnBreak { .. } => {}
            Block::Image {
                id,
                url,
                alt,
                attributes,
                position,
                ..
            } => self.image(*id, url, alt, attributes, *position),
            Block::List {
                id,
                ordered,
                start,
                tight,
                items,
                attributes,
                ..
            } => {
                let tag = if *ordered { "ol" } else { "ul" };
                let start = start.to_string();
                let extra: &[(&str, &str)] = if *ordered && *start != *"1" {
                    &[("start", &start)]
                } else {
                    &[]
                };
                self.open(tag, *id, attributes, extra);
                self.out.push('\n');
                for item in items {
                    self.item(item, *tight);
                }
                self.close(tag);
            }
            Block::Table {
                id,
                head,
                body,
                attributes,
                ..
            } => {
                self.open("table", *id, attributes, &[]);
                self.out.push('\n');
                for (group, rows, cell) in [("thead", head, "th"), ("tbody", body, "td")] {
                    if rows.is_empty() {
                        continue;
                    }
                    self.out.push('<');
                    self.out.push_str(group);
                    self.out.push_str(">\n");
                    for row in rows {
                        self.open("tr", row.id, &row.attributes, &[]);
                        for content in &row.cells {
                            let align = content.align.map(|align| match align {
                                Alignment::Left => "left",
                                Alignment::Center => "center",
                                Alignment::Right => "right",
                            });
                            let extra: &[(&str, &str)] = match &align {
                                Some(align) => &[("data-align", align)],
                                None => &[],
                            };
                            self.open(cell, content.id, &content.attributes, extra);
                            self.blocks(&content.blocks, false);
                            self.close(cell);
                        }
                        self.close("tr");
                        self.out.push('\n');
                    }
                    self.close(group);
                    self.out.push('\n');
                }
                self.close("table");
            }
        }
    }

    fn item(&mut self, item: &'s ListItem, tight: bool) {
        self.open("li", item.id, &item.attributes, &[]);
        let mut first = true;
        for block in &item.blocks {
            if !first {
                self.out.push('\n');
            }
            first = false;
            self.block(block, tight);
        }
        self.close("li");
        self.out.push('\n');
    }

    fn image(
        &mut self,
        id: NodeId,
        url: &str,
        alt: &str,
        attributes: &Attributes,
        position: Option<SourcePos>,
    ) {
        let images = self.cx.images;
        match self
            .cx
            .resources
            .resolve(url, Kind::Image, |url| images.load(url))
        {
            Ok(href) => {
                let href = href.to_string();
                self.open("img", id, attributes, &[("src", &href), ("alt", alt)]);
                self.close("img");
            }
            Err(refused) => self.warn(refusal("Image", url, refused), position),
        }
    }

    fn inlines(&mut self, inlines: &'s [Inline]) {
        for inline in inlines {
            self.inline(inline);
        }
    }

    fn inline(&mut self, inline: &'s Inline) {
        match inline {
            Inline::Text { value, .. } => xml::text(&mut self.out, value),
            Inline::Break { .. } => self.out.push_str("<br/>"),
            Inline::Code {
                id,
                value,
                attributes,
                ..
            } => {
                self.open_inline("code", *id, attributes, &[]);
                xml::text(&mut self.out, value);
                self.close("code");
            }
            Inline::Emphasis {
                id,
                children,
                attributes,
                ..
            } => self.wrap("em", *id, attributes, children),
            Inline::Strong {
                id,
                children,
                attributes,
                ..
            } => self.wrap("strong", *id, attributes, children),
            Inline::Strikethrough {
                id,
                children,
                attributes,
                ..
            } => self.wrap("s", *id, attributes, children),
            Inline::Span {
                id,
                children,
                attributes,
                ..
            } => self.wrap("span", *id, attributes, children),
            Inline::Link {
                id,
                url,
                children,
                attributes,
                position,
                ..
            } => {
                let href = match self.cx.anchors.resolve(url, self.source) {
                    LinkTarget::Outside => Some(url.clone()),
                    LinkTarget::Node(node) => self.cx.plan.href(node),
                    LinkTarget::Missing => None,
                };
                if href.is_none() {
                    self.warn(
                        format!("`{url}` names nothing in the book. The text is not a link."),
                        *position,
                    );
                }
                let extra: &[(&str, &str)] = match &href {
                    Some(href) => &[("href", href)],
                    None => &[],
                };
                self.open_inline("a", *id, attributes, extra);
                self.inlines(children);
                self.close("a");
            }
            Inline::Note { id, .. } => {
                let number = self.numbers.len() + 1;
                self.numbers.insert(*id, number);
                self.notes.push(inline);
                let target = format!("#{}", self.cx.plan.id(*id).unwrap_or_default());
                self.out
                    .push_str("<a epub:type=\"noteref\" role=\"doc-noteref\"");
                self.attribute("href", &target);
                self.out.push('>');
                self.out.push_str(&number.to_string());
                self.out.push_str("</a>");
            }
        }
    }

    fn wrap(&mut self, tag: &str, id: NodeId, attributes: &Attributes, children: &'s [Inline]) {
        self.open_inline(tag, id, attributes, &[]);
        self.inlines(children);
        self.close(tag);
    }

    /// The text of every note, after the section, in the order the
    /// references were written. A note written inside a note comes
    /// after the one that holds it.
    fn notes(&mut self) {
        let mut next = 0;
        while let Some(note) = self.notes.get(next).copied() {
            next += 1;
            let Inline::Note {
                id,
                blocks,
                attributes,
                ..
            } = note
            else {
                continue;
            };
            self.open(
                "aside",
                *id,
                attributes,
                &[("epub:type", "footnote"), ("role", "doc-footnote")],
            );
            self.out.push('\n');
            self.blocks(blocks, false);
            self.close("aside");
            self.out.push('\n');
        }
    }
}

/// The tag a heading level is written with.
fn heading(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "h1",
        HeadingLevel::H2 => "h2",
        HeadingLevel::H3 => "h3",
        HeadingLevel::H4 => "h4",
        HeadingLevel::H5 => "h5",
        HeadingLevel::H6 => "h6",
    }
}

/// The warning for a file the container could not take.
pub fn refusal(what: &str, url: &str, refused: Refused) -> String {
    match refused {
        Refused::Missing => {
            format!("{what} {url} did not load. It is left out of the EPUB.")
        }
        Refused::Unknown => {
            format!("{what} {url} is not a type an EPUB can hold. It is left out of the EPUB.")
        }
    }
}

/// One heading, for the navigation document.
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub href: String,
}

/// Every heading of the book with words in it, in reading order. A
/// heading inside a quotation, a list, a table or a note is part of
/// that and not of the book's outline.
pub fn headings(book: &Book, plan: &Plan) -> Vec<Heading> {
    let mut out = Vec::new();
    for section in &book.sections {
        for block in &section.blocks {
            let Block::Heading {
                id, level, inlines, ..
            } = block
            else {
                continue;
            };
            let title = text(inlines)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if let (false, Some(href)) = (title.is_empty(), plan.href(*id)) {
                out.push(Heading {
                    level: u8::from(*level),
                    title,
                    href,
                });
            }
        }
    }
    out
}
