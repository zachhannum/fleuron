//! Cross-references: the text the sheet generates around an inline
//! element, and the page the node a link reaches lands on.
//!
//! A page number is known after pagination, and the reference that
//! prints it is set before. So a book whose references print pages is
//! laid out twice. The first pass sets a placeholder where each page
//! number goes, and reads off the page each node landed on. The second
//! sets the numbers the first found. The second pass ships, and where
//! it moved an element the first one found, the run says so.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::Warning;
use crate::content::{
    Anchors, Block, Book, Inline, LinkTarget, NodeId, Row, Section, block_id, inline_id,
    inline_position, origin, rows, text,
};
use crate::lines::{Generated, InlineStyles, ParagraphStyle};
use crate::style::{ComputedStyle, Content, ContentPiece, StyleTree, Target};

use super::Paginator;
use super::flow::Paged;
use super::furniture::folios;

/// What a page number is set as on the pass that finds the pages:
/// three figures, as wide as most page numbers a book prints.
const PLACEHOLDER: &str = "000";

/// What the references in one book resolve against.
#[derive(Debug, Clone, Default)]
pub(crate) struct References {
    /// What the links reach. `None` where the book is not known, so
    /// nothing can be missing from it.
    known: Option<Arc<Known>>,
    /// The folio each node landed on in the pass before this one.
    /// `None` on the pass that finds them.
    folios: Option<Arc<BTreeMap<NodeId, u32>>>,
}

#[derive(Debug)]
struct Known {
    anchors: Anchors,
    /// The text of each node a link can reach.
    texts: BTreeMap<NodeId, String>,
}

impl References {
    /// What the links in one book reach, each node with its text.
    pub(crate) fn of(book: &Book) -> References {
        let anchors = book.anchors();
        let mut texts = BTreeMap::new();
        for section in &book.sections {
            if anchors.reaches(section.id) {
                texts.insert(section.id, section_text(section));
            }
            texts_of(&section.blocks, &anchors, &mut texts);
        }
        References {
            known: Some(Arc::new(Known { anchors, texts })),
            folios: None,
        }
    }

    /// The same, with the folio each node landed on.
    pub(crate) fn landed(&self, folios: BTreeMap<NodeId, u32>) -> References {
        References {
            known: self.known.clone(),
            folios: Some(Arc::new(folios)),
        }
    }

    /// The text of one node a link can reach.
    pub(crate) fn text(&self, node: NodeId) -> Option<&str> {
        self.known.as_ref()?.texts.get(&node).map(String::as_str)
    }

    /// What a warning calls one node a link can reach.
    fn name(&self, node: NodeId) -> String {
        self.known
            .as_ref()
            .and_then(|known| known.anchors.name(node))
            .map_or_else(|| format!("node {}", node.get()), str::to_string)
    }

    /// The text one `content` value generates on an element whose
    /// `href` is `href`, written in `source`, or why it generates none.
    fn resolve(
        &self,
        content: &Content,
        href: Option<&str>,
        source: Option<&str>,
    ) -> Result<String, Unresolved> {
        let pieces = match content {
            Content::Text(text) => return Ok(text.clone()),
            Content::Pieces(pieces) => pieces,
            _ => return Ok(String::new()),
        };
        let mut generated = String::new();
        for piece in pieces {
            match piece {
                ContentPiece::Text(text) => generated.push_str(text),
                ContentPiece::TargetCounter { target, style } => {
                    let node = self.target(target, href, source)?;
                    match (&self.folios, node) {
                        (Some(folios), Some(node)) => {
                            let folio = folios
                                .get(&node)
                                .ok_or_else(|| Unresolved::Unplaced(self.name(node)))?;
                            generated.push_str(&style.format(*folio));
                        }
                        _ => generated.push_str(PLACEHOLDER),
                    }
                }
                ContentPiece::TargetText { target } => {
                    if let Some(node) = self.target(target, href, source)? {
                        generated.push_str(self.text(node).unwrap_or_default());
                    }
                }
            }
        }
        Ok(generated)
    }

    /// The node one target reaches from `source`. `None` where the
    /// book is not known.
    fn target(
        &self,
        target: &Target,
        href: Option<&str>,
        source: Option<&str>,
    ) -> Result<Option<NodeId>, Unresolved> {
        let url = target.url(href).ok_or(Unresolved::NotALink)?;
        let Some(known) = &self.known else {
            return Ok(None);
        };
        match known.anchors.resolve(url, source) {
            LinkTarget::Node(node) => Ok(Some(node)),
            LinkTarget::Outside => Err(Unresolved::Outside),
            LinkTarget::Missing => Err(Unresolved::Missing(url.to_string())),
        }
    }
}

/// Why a reference generates nothing.
enum Unresolved {
    /// `attr(href url)` on an element that is not a link.
    NotALink,
    /// A url outside the book, which a sheet that styles every link
    /// meets on purpose.
    Outside,
    /// A url that reaches nothing in the book.
    Missing(String),
    /// A node that reached no page, by its name.
    Unplaced(String),
}

impl Unresolved {
    fn message(&self) -> Option<String> {
        Some(match self {
            Unresolved::NotALink => "`attr(href url)` is used on an element that is not a link. \
                                     Nothing is generated."
                .to_string(),
            Unresolved::Outside => return None,
            Unresolved::Missing(url) => {
                format!("`{url}` names nothing in the book. Nothing is generated.")
            }
            Unresolved::Unplaced(name) => {
                format!("`{name}` is on no page. Nothing is generated.")
            }
        })
    }
}

/// Takes the text of one node a link can reach, unless an element
/// before it took the node already.
fn claim(
    texts: &mut BTreeMap<NodeId, String>,
    anchors: &Anchors,
    node: NodeId,
    text: impl FnOnce() -> String,
) {
    if anchors.reaches(node) {
        texts.entry(node).or_insert_with(text);
    }
}

/// The text of every node a link can reach, in these blocks and
/// everything inside them.
fn texts_of(blocks: &[Block], anchors: &Anchors, texts: &mut BTreeMap<NodeId, String>) {
    for block in blocks {
        claim(texts, anchors, block_id(block), || block_text(block));
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                inline_texts(inlines, anchors, texts)
            }
            Block::Blockquote { blocks, .. } => texts_of(blocks, anchors, texts),
            Block::Table { head, body, .. } => {
                for row in rows(head, body) {
                    claim(texts, anchors, row.id, || row_text(row));
                    for cell in &row.cells {
                        claim(texts, anchors, cell.id, || blocks_text(&cell.blocks));
                        texts_of(&cell.blocks, anchors, texts);
                    }
                }
            }
            Block::ThematicBreak { .. } | Block::Image { .. } => {}
        }
    }
}

/// The same, over the inlines of one block.
fn inline_texts(inlines: &[Inline], anchors: &Anchors, texts: &mut BTreeMap<NodeId, String>) {
    for inline in inlines {
        claim(texts, anchors, inline_id(inline), || {
            text(std::slice::from_ref(inline))
        });
        if let Inline::Emphasis { children, .. }
        | Inline::Strong { children, .. }
        | Inline::Link { children, .. } = inline
        {
            inline_texts(children, anchors, texts);
        }
    }
}

/// What `target-text()` prints for a link to a whole source: the
/// section's title, or the words of the heading it opens with.
fn section_text(section: &Section) -> String {
    section
        .title
        .clone()
        .unwrap_or_else(|| match section.blocks.first() {
            Some(block @ Block::Heading { .. }) => block_text(block),
            _ => String::new(),
        })
}

/// What `target-text()` prints for one block: its words, markup
/// discarded, or an image's description.
fn block_text(block: &Block) -> String {
    match block {
        Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => text(inlines),
        Block::Image { alt, .. } => alt.clone(),
        Block::Blockquote { blocks, .. } => blocks_text(blocks),
        Block::Table { head, body, .. } => {
            rows(head, body).map(row_text).collect::<Vec<_>>().join(" ")
        }
        Block::ThematicBreak { .. } => String::new(),
    }
}

/// The words of one table row, a space between each two cells.
fn row_text(row: &Row) -> String {
    row.cells
        .iter()
        .map(|cell| blocks_text(&cell.blocks))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The words of several blocks, a space between each two.
fn blocks_text(blocks: &[Block]) -> String {
    blocks.iter().map(block_text).collect::<Vec<_>>().join(" ")
}

/// The nodes one section's references reach, in the order they are
/// written.
#[derive(Debug, Default)]
pub(crate) struct Named {
    /// Those whose page a reference prints.
    pub(crate) pages: Vec<NodeId>,
    /// Those whose text a reference prints.
    pub(crate) texts: Vec<NodeId>,
}

impl Named {
    /// What the references in one section reach.
    pub(crate) fn in_section(
        section: &Section,
        styles: &StyleTree,
        references: &References,
    ) -> Named {
        let mut named = Named::default();
        let source = section.source.as_deref();
        named.blocks(&section.blocks, styles, references, source);
        named
    }

    fn blocks(
        &mut self,
        blocks: &[Block],
        styles: &StyleTree,
        references: &References,
        source: Option<&str>,
    ) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    self.inlines(inlines, styles, references, source)
                }
                Block::Blockquote { blocks, .. } => self.blocks(blocks, styles, references, source),
                Block::Table { head, body, .. } => {
                    for cell in rows(head, body).flat_map(|row| &row.cells) {
                        self.blocks(&cell.blocks, styles, references, source);
                    }
                }
                Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }

    fn inlines(
        &mut self,
        inlines: &[Inline],
        styles: &StyleTree,
        references: &References,
        source: Option<&str>,
    ) {
        for inline in inlines {
            let (href, children) = match inline {
                Inline::Text { .. } | Inline::Break { .. } => continue,
                Inline::Code { .. } => (None, None),
                Inline::Emphasis { children, .. } | Inline::Strong { children, .. } => {
                    (None, Some(children))
                }
                Inline::Link { url, children, .. } => (Some(url.as_str()), Some(children)),
            };
            let id = inline_id(inline);
            for pseudo in [styles.before(id), styles.after(id)].into_iter().flatten() {
                let Content::Pieces(pieces) = &pseudo.content else {
                    continue;
                };
                for piece in pieces {
                    let (list, target) = match piece {
                        ContentPiece::Text(_) => continue,
                        ContentPiece::TargetCounter { target, .. } => (&mut self.pages, target),
                        ContentPiece::TargetText { target } => (&mut self.texts, target),
                    };
                    if let Ok(Some(node)) = references.target(target, href, source) {
                        list.push(node);
                    }
                }
            }
            if let Some(children) = children {
                self.inlines(children, styles, references, source);
            }
        }
    }
}

/// The folio each node landed on, off one pass's pages.
pub(crate) fn landed(paged: &Paged) -> BTreeMap<NodeId, u32> {
    let numbers = folios(&paged.infos);
    paged
        .targets
        .iter()
        .map(|(node, index)| (*node, numbers[*index]))
        .collect()
}

/// A warning for every node a reference prints the page of that the
/// second pass set on another page than the first found it on.
pub(crate) fn moved(
    found: &BTreeMap<NodeId, u32>,
    landed: &BTreeMap<NodeId, u32>,
    printed: &BTreeSet<NodeId>,
    references: &References,
) -> Vec<Warning> {
    printed
        .iter()
        .filter_map(|node| {
            let (was, now) = (found.get(node)?, landed.get(node)?);
            (was != now).then(|| Warning {
                message: format!(
                    "The page printed for `{}` is {was}, and the element is on page {now}.",
                    references.name(*node),
                ),
                origin: None,
            })
        })
        .collect()
}

/// The style tree, with the text each pseudo-element generates
/// resolved against the book's references. Line layout asks this for
/// the style of every inline of a paragraph.
pub(super) struct Referring<'r, 'a> {
    pub(super) paginator: &'r Paginator<'a>,
    /// The file the paragraph was read from, for diagnostics and for
    /// the links that name no file.
    pub(super) source: Option<&'r str>,
}

impl InlineStyles for Referring<'_, '_> {
    fn style(&self, id: NodeId, block: ParagraphStyle) -> ParagraphStyle {
        InlineStyles::style(self.paginator.styles, id, block)
    }

    fn generated(&self, inline: &Inline) -> Generated {
        let styles = self.paginator.styles;
        let id = inline_id(inline);
        let href = match inline {
            Inline::Link { url, .. } => Some(url.as_str()),
            _ => None,
        };
        let text = |pseudo: Option<&ComputedStyle>| {
            let pseudo = pseudo?;
            let resolved =
                self.paginator
                    .references
                    .borrow()
                    .resolve(&pseudo.content, href, self.source);
            match resolved {
                Ok(text) => Some((text, pseudo.paragraph())),
                Err(unresolved) => {
                    if let Some(message) = unresolved.message() {
                        let at = origin(self.source, inline_position(inline));
                        self.paginator.warn(message, (!at.is_empty()).then_some(at));
                    }
                    None
                }
            }
        };
        Generated {
            before: text(styles.before(id)),
            after: text(styles.after(id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Attributes, HeadingLevel, SourcePos};
    use crate::layout::testing::{book_of, heading, long_prose, registry, section, styled};
    use crate::layout::{layout_book, no_assets};
    use crate::pages::{DrawItem, Page};
    use crate::style::CounterStyle;

    fn words(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    /// A link written on `line`, carrying `classes`.
    fn link(url: &str, value: &str, line: u32, classes: &[&str]) -> Inline {
        Inline::Link {
            id: NodeId::UNASSIGNED,
            url: url.into(),
            children: vec![words(value)],
            attributes: Attributes {
                id: None,
                classes: classes.iter().map(|class| class.to_string()).collect(),
            },
            position: Some(SourcePos { line, column: 5 }),
            span: None,
        }
    }

    fn named(id: &str) -> Attributes {
        Attributes {
            id: Some(id.into()),
            classes: Vec::new(),
        }
    }

    /// A heading that carries `id`.
    fn named_heading(title: &str, id: &str) -> Block {
        Block::Heading {
            id: NodeId::UNASSIGNED,
            level: HeadingLevel::H1,
            inlines: vec![words(title)],
            attributes: named(id),
            position: None,
            span: None,
        }
    }

    fn paragraph_of(inlines: Vec<Inline>) -> Block {
        Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines,
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    /// A chapter that opens on a heading carrying `id`, with a link
    /// to `to` in its first paragraph and pages of prose after it.
    fn chapter(title: &str, id: &str, to: &str, name: &str) -> Section {
        let mut blocks = vec![
            named_heading(title, id),
            paragraph_of(vec![
                words("See "),
                link(to, name, 3, &[]),
                words(" for the rest."),
            ]),
        ];
        blocks.extend(long_prose(30));
        section(blocks)
    }

    /// Three chapters of several pages each. The first refers to the
    /// third, the second to itself, and the third to the first.
    fn cross_referenced() -> Book {
        book_of(vec![
            chapter("The Voyage", "the-voyage", "#the-hunter", "the hunter"),
            chapter("The Storm", "the-storm", "#the-storm", "the storm"),
            chapter("The Hunter", "the-hunter", "#the-voyage", "the voyage"),
        ])
    }

    const PAGE_REFERENCE: &str =
        "a::after { content: \" (page \" target-counter(attr(href url), page) \")\" }";

    /// The pages `css` lays `book` out to, what the run complained
    /// about, and how many times it laid the book out again.
    fn lay_out(css: &str, book: &Book) -> (Vec<Page>, Vec<Warning>, u32) {
        let styles = styled(css, book);
        let paginator = Paginator::new(registry(), &styles);
        let pages = paginator.paginate(book);
        (pages, paginator.warnings(), paginator.settles())
    }

    /// The words of one page as they were written, runs joined and
    /// spaces squeezed.
    fn page_words(page: &Page) -> String {
        let runs: Vec<&str> = page
            .items
            .iter()
            .filter_map(|item| match item {
                DrawItem::Text { text, source, .. } if source.is_empty() => Some(text.as_str()),
                DrawItem::Text { source, .. } => Some(source.as_str()),
                _ => None,
            })
            .collect();
        runs.join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The first page whose words hold `words`.
    fn page_with<'p>(pages: &'p [Page], words: &str) -> &'p Page {
        pages
            .iter()
            .find(|page| page_words(page).contains(words))
            .unwrap_or_else(|| panic!("no page holds {words:?}"))
    }

    /// Acceptance: a link to `#the-hunter` prints the page that
    /// heading landed on. A reference to a page before it prints that
    /// page, and a chapter that names itself prints the page it opens
    /// on.
    #[test]
    fn a_link_prints_the_page_its_target_landed_on() {
        let book = cross_referenced();
        let (pages, warnings, settles) = lay_out(PAGE_REFERENCE, &book);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(settles, 1);

        let folio = |title: &str| page_with(&pages, title).number;
        let (voyage, storm, hunter) =
            (folio("The Voyage"), folio("The Storm"), folio("The Hunter"));
        assert!(
            voyage < storm && storm < hunter && hunter - voyage > 4,
            "the chapters are pages apart: {voyage}, {storm}, {hunter}",
        );
        for (from, to) in [
            ("the hunter", hunter),
            ("the storm", storm),
            ("the voyage", voyage),
        ] {
            let printed = format!("See {from} (page {to}) for the rest.");
            let page = page_with(&pages, &format!("See {from}"));
            assert!(page_words(page).contains(&printed), "{}", page_words(page));
        }
    }

    /// Generated text is text the engine wrote. Its runs name no node,
    /// and they are set in the pseudo-element's own style.
    #[test]
    fn generated_text_names_no_node_and_takes_its_own_style() {
        let book = cross_referenced();
        let css = format!("{PAGE_REFERENCE} a::after {{ font-size: 7pt }}");
        let (pages, ..) = lay_out(&css, &book);
        let generated: Vec<(&str, f32, bool)> = pages
            .iter()
            .flat_map(|page| &page.items)
            .filter_map(|item| match item {
                DrawItem::Text {
                    text, size, origin, ..
                } if text.contains("(page") => Some((text.as_str(), *size, origin.is_some())),
                _ => None,
            })
            .collect();
        assert_eq!(generated.len(), 3, "{generated:?}");
        for (text, size, named) in generated {
            assert_eq!(size, 7.0, "{text:?} is not in the pseudo-element's size");
            assert!(!named, "{text:?} names a node");
        }
    }

    /// Acceptance: `target-counter(attr(href url), page, upper-roman)`
    /// prints the folio in the style the front matter uses. A folio is
    /// what a page prints, so a count that restarts restarts the
    /// number a reference prints.
    #[test]
    fn a_reference_prints_the_folio_in_the_style_it_names() {
        let mut preface = vec![named_heading("Preface", "preface")];
        preface.extend(long_prose(48));
        preface.push(named_heading("Acknowledgements", "thanks"));
        preface.extend(long_prose(2));
        let mut one = vec![
            named_heading("One", "one"),
            paragraph_of(vec![
                words("As the "),
                link("#thanks", "acknowledgements", 3, &["front"]),
                words(" say, and "),
                link("#two", "chapter two", 3, &[]),
                words(" shows."),
            ]),
        ];
        one.extend(long_prose(20));
        let mut two = vec![named_heading("Two", "two")];
        two.extend(long_prose(4));
        let book = book_of(vec![section(preface), section(one), section(two)]);
        let css = "section:first-child { page: front }
             @page front { @bottom-center { content: counter(page, upper-roman) } }
             section:nth-child(2) { counter-reset: page 1 }
             a.front::after { content: \" (\" target-counter(attr(href url), page, upper-roman) \")\" }
             a:not(.front)::after { content: \" (\" target-counter(attr(href url), page) \")\" }";
        let (pages, warnings, _) = lay_out(css, &book);
        assert!(warnings.is_empty(), "{warnings:?}");

        let thanks = page_with(&pages, "Acknowledgements").number;
        assert!(thanks >= 3, "the acknowledgements open deep in the preface");
        let two_at = pages
            .iter()
            .position(|page| page_words(page).contains("Two"))
            .expect("chapter two is set");
        let two = pages[two_at].number;
        assert_ne!(two, two_at as u32 + 1, "the count restarted");

        let printed = format!(
            "As the acknowledgements ({}) say, and chapter two ({two}) shows.",
            CounterStyle::UpperRoman.format(thanks),
        );
        let page = page_with(&pages, "As the acknowledgements");
        assert!(page_words(page).contains(&printed), "{}", page_words(page));
    }

    /// Acceptance: `target-text()` prints the heading's own words. They
    /// are in the book before anything is laid out, so the book is
    /// laid out once.
    #[test]
    fn target_text_prints_the_words_of_its_target() {
        let book = cross_referenced();
        let css = "a::after { content: \" (\" target-text(attr(href url)) \")\" }";
        let (pages, warnings, settles) = lay_out(css, &book);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(settles, 0);
        let page = page_with(&pages, "See the hunter");
        assert!(
            page_words(page).contains("See the hunter (The Hunter) for the rest."),
            "{}",
            page_words(page),
        );
    }

    /// `target-text()` prints a heading with a hard break as the words
    /// of one line, with a space where the break was. The reference
    /// sits on the line of the paragraph it is generated in.
    #[test]
    fn target_text_prints_a_broken_heading_on_one_line() {
        let voyage = Block::Heading {
            id: NodeId::UNASSIGNED,
            level: HeadingLevel::H1,
            inlines: vec![
                words("Chapter One"),
                Inline::Break {
                    id: NodeId::UNASSIGNED,
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                },
                words("The Voyage"),
            ],
            attributes: Attributes {
                id: Some("the-voyage".into()),
                classes: Vec::new(),
            },
            position: None,
            span: None,
        };
        let mut chapter = section(vec![
            voyage,
            paragraph_of(vec![
                words("See "),
                link("#the-voyage", "there", 12, &[]),
                words(" for the rest."),
            ]),
        ]);
        chapter.source = Some("one.md".into());
        let book = book_of(vec![chapter]);
        let css = "a::after { content: \" (\" target-text(attr(href url)) \")\" }";
        let (pages, warnings, _) = lay_out(css, &book);
        assert!(warnings.is_empty(), "{warnings:?}");
        let page = page_with(&pages, "See there");
        assert!(
            page_words(page).contains("See there (Chapter One The Voyage) for the rest."),
            "{}",
            page_words(page),
        );
        let baseline = |words: &str| {
            page.items
                .iter()
                .find_map(|item| match item {
                    DrawItem::Text { text, y, .. } if text.contains(words) => Some(*y),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{words:?} is not drawn"))
        };
        assert_eq!(baseline("See"), baseline("Voyage)"));
    }

    /// A one-chapter book whose only paragraph links to `url`,
    /// written at line 12 of `one.md`.
    fn linking_to(url: &str) -> Book {
        let mut chapter = section(vec![
            named_heading("The Voyage", "the-voyage"),
            paragraph_of(vec![
                words("See "),
                Inline::Emphasis {
                    id: NodeId::UNASSIGNED,
                    children: vec![link(url, "elsewhere", 12, &[])],
                    attributes: Attributes::default(),
                    position: Some(SourcePos {
                        line: 12,
                        column: 4,
                    }),
                    span: None,
                },
                words(" for the rest."),
            ]),
        ]);
        chapter.source = Some("one.md".into());
        book_of(vec![chapter])
    }

    /// Acceptance: a reference to a heading or a file that the book
    /// does not have warns, naming the line and column it was written
    /// at. It generates nothing, and the run goes on.
    #[test]
    fn a_reference_to_nothing_in_the_book_warns_and_generates_nothing() {
        for url in ["#nowhere", "two.md#the-voyage", "two.md"] {
            let (pages, warnings, _) = lay_out(PAGE_REFERENCE, &linking_to(url));
            assert_eq!(
                warnings,
                [Warning {
                    message: format!("`{url}` names nothing in the book. Nothing is generated."),
                    origin: Some("one.md:12:5".into()),
                }],
            );
            assert!(page_words(&pages[0]).contains("See elsewhere for the rest."));
            assert!(!page_words(&pages[0]).contains("(page"));
        }
    }

    /// Acceptance: a link to something outside the book prints nothing
    /// and does not warn.
    #[test]
    fn a_link_outside_the_book_prints_nothing_quietly() {
        for url in ["https://example.com", "mailto:someone@example.com"] {
            let (pages, warnings, _) = lay_out(PAGE_REFERENCE, &linking_to(url));
            assert!(warnings.is_empty(), "{url}: {warnings:?}");
            assert!(page_words(&pages[0]).contains("See elsewhere for the rest."));
        }
    }

    /// `attr(href url)` on an element that is not a link generates
    /// nothing, and says why. A url the sheet writes names its target
    /// outright.
    #[test]
    fn a_reference_on_an_element_that_is_not_a_link_warns() {
        let css = "em::after { content: target-counter(attr(href url), page) }";
        let (_, warnings, _) = lay_out(css, &linking_to("#the-voyage"));
        assert_eq!(
            warnings,
            [Warning {
                message: "`attr(href url)` is used on an element that is not a link. \
                          Nothing is generated."
                    .into(),
                origin: Some("one.md:12:4".into()),
            }],
        );

        let css = "em::after { content: \" (page \" target-counter(\"#the-voyage\", page) \")\" }";
        let (pages, warnings, _) = lay_out(css, &linking_to("#the-voyage"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(page_words(&pages[0]).contains("See elsewhere (page 1) for the rest."));
    }

    /// Acceptance: laid out twice, a book with references comes out
    /// byte for byte the same, over the same number of pages.
    #[test]
    fn a_book_with_references_lays_out_the_same_twice() {
        for book in [cross_referenced(), chapter_files()] {
            let styles = styled(PAGE_REFERENCE, &book);
            let run = || layout_book(&book, &styles, registry(), no_assets());
            let (first, second) = (run(), run());
            assert_eq!(first.pages.len(), second.pages.len());
            assert_eq!(
                crate::wire::encode(&first).expect("the output encodes"),
                crate::wire::encode(&second).expect("the output encodes"),
            );
        }
    }

    /// Acceptance: a book with no reference to a page lays out in one
    /// pass. Text generated around a link, or the words of the element
    /// it names, do not change that.
    #[test]
    fn only_a_page_reference_lays_the_book_out_twice() {
        for (css, settles) in [
            ("", 0),
            ("a::after { content: \" *\" }", 0),
            ("a::after { content: target-text(attr(href url)) }", 0),
            (PAGE_REFERENCE, 1),
        ] {
            assert_eq!(lay_out(css, &cross_referenced()).2, settles, "{css:?}");
        }
    }

    /// An id on an element inside a paragraph lands on the page the
    /// paragraph opens on.
    #[test]
    fn an_id_inside_a_paragraph_is_a_target() {
        let mut book = cross_referenced();
        let marked = paragraph_of(vec![
            words("The "),
            Inline::Emphasis {
                id: NodeId::UNASSIGNED,
                children: vec![words("mark")],
                attributes: named("the-mark"),
                position: None,
                span: None,
            },
            words(" is here."),
        ]);
        book.sections[2].blocks.push(marked);
        book.sections[0].blocks[1] = paragraph_of(vec![link("#the-mark", "the mark", 3, &[])]);
        book.assign_node_ids();
        let (pages, warnings, _) = lay_out(PAGE_REFERENCE, &book);
        assert!(warnings.is_empty(), "{warnings:?}");
        let at = page_with(&pages, "The mark is here.").number;
        assert!(page_words(&pages[0]).contains(&format!("the mark (page {at})")));
    }

    /// A chapter read from `source` that opens on a heading with no id
    /// written, with the links in its first paragraph, pages of prose,
    /// and a closing heading `Notes` over one line of its own.
    fn chapter_file(source: &str, title: &str, links: &[(&str, &str)]) -> Section {
        let mut inlines = vec![words("See")];
        for (url, name) in links {
            inlines.push(words(" "));
            inlines.push(link(url, name, 3, &[]));
        }
        let mut blocks = vec![heading(title), paragraph_of(inlines)];
        blocks.extend(long_prose(30));
        blocks.push(heading("Notes"));
        blocks.push(paragraph_of(vec![words(&format!("Notes to {title}."))]));
        let mut chapter = section(blocks);
        chapter.source = Some(source.into());
        chapter
    }

    /// Three chapters, one note each, as an Obsidian vault keeps them.
    /// The first links to the third in every form a link can take, and
    /// each links to its own notes.
    fn chapter_files() -> Book {
        book_of(vec![
            chapter_file(
                "Chapter 1.md",
                "The Voyage",
                &[
                    ("Chapter%203.md#the-hunter", "slug"),
                    ("Chapter%203.md#The%20Hunter", "encoded"),
                    ("Chapter 3#The Hunter", "wikilink"),
                    ("chapter 3", "file"),
                    ("#notes", "first notes"),
                ],
            ),
            chapter_file("Chapter 2.md", "The Storm", &[("#Notes", "second notes")]),
            chapter_file("Chapter 3.md", "The Hunter", &[("#notes", "third notes")]),
        ])
    }

    /// Acceptance: a link that names a file and a heading prints the
    /// page of that heading, in slug form, in text form, and as a
    /// wikilink. A link that names only the file prints the page the
    /// file starts on.
    #[test]
    fn a_link_names_a_heading_in_a_file() {
        let book = chapter_files();
        let (pages, warnings, _) = lay_out(PAGE_REFERENCE, &book);
        assert!(warnings.is_empty(), "{warnings:?}");
        let hunter = page_with(&pages, "See third notes").number;
        let printed: String = ["slug", "encoded", "wikilink", "file"]
            .map(|name| format!(" {name} (page {hunter})"))
            .concat();
        let page = page_with(&pages, "See slug");
        assert!(page_words(page).contains(&printed), "{}", page_words(page));
    }

    /// Acceptance: `#notes` reaches the heading in the source the link
    /// is written in. Three chapters each have one, and nothing warns.
    #[test]
    fn a_link_with_no_file_reaches_its_own_source() {
        let book = chapter_files();
        let (pages, warnings, _) = lay_out(PAGE_REFERENCE, &book);
        assert!(warnings.is_empty(), "{warnings:?}");
        for (link, title) in [
            ("first notes", "The Voyage"),
            ("second notes", "The Storm"),
            ("third notes", "The Hunter"),
        ] {
            let notes = page_with(&pages, &format!("Notes to {title}.")).number;
            let page = page_with(&pages, &format!("{link} (page"));
            assert!(
                page_words(page).contains(&format!("{link} (page {notes})")),
                "{}",
                page_words(page),
            );
        }
    }

    /// Acceptance: a heading anchor is not a CSS id. A sheet that names
    /// `#the-hunter` or `#notes` reaches no heading.
    #[test]
    fn a_heading_anchor_is_not_a_css_id() {
        let book = chapter_files();
        let css = "#the-hunter, #notes { font-size: 30pt }";
        let (pages, _, _) = lay_out(css, &book);
        let large = pages
            .iter()
            .flat_map(|page| &page.items)
            .any(|item| matches!(item, DrawItem::Text { size, .. } if *size == 30.0));
        assert!(!large, "a heading took the size");
    }

    /// The second pass ships. A node a reference prints the page of
    /// that the second pass set on another page is named.
    #[test]
    fn an_element_the_second_pass_moved_is_named() {
        let book = chapter_files();
        let references = References::of(&book);
        let hunter = block_id(&book.sections[2].blocks[0]);
        let notes = block_id(&book.sections[2].blocks[32]);
        let voyage = block_id(&book.sections[0].blocks[0]);
        let folios = |pairs: [(NodeId, u32); 3]| pairs.into_iter().collect::<BTreeMap<_, _>>();
        let found = folios([(hunter, 12), (notes, 99), (voyage, 3)]);
        let landed = folios([(hunter, 13), (notes, 99), (voyage, 4)]);
        let printed = BTreeSet::from([hunter, notes]);
        assert_eq!(
            moved(&found, &landed, &printed, &references),
            [Warning {
                message: "The page printed for `Chapter 3.md#the-hunter` is 12, and the \
                          element is on page 13."
                    .into(),
                origin: None,
            }],
        );
    }
}
