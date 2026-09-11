//! Cross-references: the text the sheet generates around an inline
//! element, and the page the element it names lands on.
//!
//! A page number is known after pagination, and the reference that
//! prints it is set before. So a book whose references print pages is
//! laid out twice. The first pass sets a placeholder where each page
//! number goes, and reads off the page each id landed on. The second
//! sets the numbers the first found. The second pass ships, and where
//! it moved an element the first one found, the run says so.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::Warning;
use crate::content::{
    Block, Book, Inline, NodeId, Row, Section, block_attributes, inline_attributes, inline_id,
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
    /// The text of each element an id names, by that id. `None` where
    /// the book is not known, so nothing can be missing from it.
    texts: Option<Arc<BTreeMap<String, String>>>,
    /// The folio each id landed on in the pass before this one.
    /// `None` on the pass that finds them.
    folios: Option<Arc<BTreeMap<String, u32>>>,
}

impl References {
    /// The ids of one book, each with its element's text. The first
    /// element to take an id keeps it.
    pub(crate) fn of(book: &Book) -> References {
        let mut texts = BTreeMap::new();
        for section in &book.sections {
            texts_of(&section.blocks, &mut texts);
        }
        References {
            texts: Some(Arc::new(texts)),
            folios: None,
        }
    }

    /// The same, with the folio each id landed on.
    pub(crate) fn landed(&self, folios: BTreeMap<String, u32>) -> References {
        References {
            texts: self.texts.clone(),
            folios: Some(Arc::new(folios)),
        }
    }

    /// The text of the element one id names.
    pub(crate) fn text(&self, id: &str) -> Option<&str> {
        self.texts.as_ref()?.get(id).map(String::as_str)
    }

    /// The text one `content` value generates on an element whose
    /// `href` is `href`, or why it generates none.
    fn resolve(&self, content: &Content, href: Option<&str>) -> Result<String, Unresolved> {
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
                    let id = self.target(target, href)?;
                    match &self.folios {
                        None => generated.push_str(PLACEHOLDER),
                        Some(folios) => {
                            let folio = folios
                                .get(id)
                                .ok_or_else(|| Unresolved::Unplaced(id.to_string()))?;
                            generated.push_str(&style.format(*folio));
                        }
                    }
                }
                ContentPiece::TargetText { target } => {
                    let id = self.target(target, href)?;
                    generated.push_str(self.text(id).unwrap_or_default());
                }
            }
        }
        Ok(generated)
    }

    /// The id one target names, where an element carries it.
    fn target<'t>(&self, target: &'t Target, href: Option<&'t str>) -> Result<&'t str, Unresolved> {
        let Some(id) = target.id(href) else {
            return Err(match (target, href) {
                (Target::Href, None) => Unresolved::NotALink,
                (Target::Href, Some(url)) => Unresolved::NotAnId(url.to_string()),
                (Target::Url(url), _) => Unresolved::NotAnId(url.clone()),
            });
        };
        match &self.texts {
            Some(texts) if !texts.contains_key(id) => Err(Unresolved::Missing(id.to_string())),
            _ => Ok(id),
        }
    }
}

/// Why a reference generates nothing.
enum Unresolved {
    /// `attr(href url)` on an element that is not a link.
    NotALink,
    /// A url that does not name an id.
    NotAnId(String),
    /// An id no element carries.
    Missing(String),
    /// An id whose element reached no page.
    Unplaced(String),
}

impl Unresolved {
    fn message(&self) -> String {
        match self {
            Unresolved::NotALink => "`attr(href url)` is used on an element that is not a link. \
                                     Nothing is generated."
                .to_string(),
            Unresolved::NotAnId(url) => {
                format!("`{url}` is not an id in the book. Nothing is generated.")
            }
            Unresolved::Missing(id) => {
                format!("No element has the id `{id}`. Nothing is generated.")
            }
            Unresolved::Unplaced(id) => {
                format!("The element with the id `{id}` is on no page. Nothing is generated.")
            }
        }
    }
}

/// Takes one id for the element that carries it, unless an element
/// before it took the id already.
fn claim(texts: &mut BTreeMap<String, String>, id: &Option<String>, text: impl FnOnce() -> String) {
    if let Some(id) = id {
        texts.entry(id.clone()).or_insert_with(text);
    }
}

/// The text of every element an id names, in these blocks and
/// everything inside them.
fn texts_of(blocks: &[Block], texts: &mut BTreeMap<String, String>) {
    for block in blocks {
        claim(texts, &block_attributes(block).id, || block_text(block));
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                inline_texts(inlines, texts)
            }
            Block::Blockquote { blocks, .. } => texts_of(blocks, texts),
            Block::Table { head, body, .. } => {
                for row in rows(head, body) {
                    claim(texts, &row.attributes.id, || row_text(row));
                    for cell in &row.cells {
                        claim(texts, &cell.attributes.id, || blocks_text(&cell.blocks));
                        texts_of(&cell.blocks, texts);
                    }
                }
            }
            Block::ThematicBreak { .. } | Block::Image { .. } => {}
        }
    }
}

/// The same, over the inlines of one block.
fn inline_texts(inlines: &[Inline], texts: &mut BTreeMap<String, String>) {
    for inline in inlines {
        claim(texts, &inline_attributes(inline).id, || {
            text(std::slice::from_ref(inline))
        });
        if let Inline::Emphasis { children, .. }
        | Inline::Strong { children, .. }
        | Inline::Link { children, .. } = inline
        {
            inline_texts(children, texts);
        }
    }
}

/// What `target-text()` prints for one block: its words, markup
/// discarded, or an image's description.
fn block_text(block: &Block) -> String {
    match block {
        Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => text(inlines),
        Block::Image { alt, .. } => alt.clone(),
        Block::Blockquote { blocks, .. } => blocks_text(blocks),
        Block::Table { head, body, .. } => rows(head, body)
            .map(row_text)
            .collect::<Vec<_>>()
            .join(" "),
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

/// The ids one section's references name, in the order they are
/// written.
#[derive(Debug, Default)]
pub(crate) struct Named {
    /// Those whose page a reference prints.
    pub(crate) pages: Vec<String>,
    /// Those whose text a reference prints.
    pub(crate) texts: Vec<String>,
}

impl Named {
    /// What the references in one section name.
    pub(crate) fn in_section(section: &Section, styles: &StyleTree) -> Named {
        let mut named = Named::default();
        named.blocks(&section.blocks, styles);
        named
    }

    fn blocks(&mut self, blocks: &[Block], styles: &StyleTree) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    self.inlines(inlines, styles)
                }
                Block::Blockquote { blocks, .. } => self.blocks(blocks, styles),
                Block::Table { head, body, .. } => {
                    for cell in rows(head, body).flat_map(|row| &row.cells) {
                        self.blocks(&cell.blocks, styles);
                    }
                }
                Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }

    fn inlines(&mut self, inlines: &[Inline], styles: &StyleTree) {
        for inline in inlines {
            let (href, children) = match inline {
                Inline::Text { .. } => continue,
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
                    if let Some(id) = target.id(href) {
                        list.push(id.to_string());
                    }
                }
            }
            if let Some(children) = children {
                self.inlines(children, styles);
            }
        }
    }
}

/// The folio each id landed on, off one pass's pages.
pub(crate) fn landed(paged: &Paged) -> BTreeMap<String, u32> {
    let numbers = folios(&paged.infos);
    paged
        .targets
        .iter()
        .map(|(id, index)| (id.clone(), numbers[*index]))
        .collect()
}

/// A warning for every element a reference prints the page of that
/// the second pass set on another page than the first found it on.
pub(crate) fn moved(
    found: &BTreeMap<String, u32>,
    landed: &BTreeMap<String, u32>,
    printed: &BTreeSet<String>,
) -> Vec<Warning> {
    printed
        .iter()
        .filter_map(|id| {
            let (was, now) = (found.get(id)?, landed.get(id)?);
            (was != now).then(|| Warning {
                message: format!(
                    "The page printed for `{id}` is {was}, and the element is on page {now}."
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
    /// The file the paragraph was read from, for diagnostics.
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
            let resolved = self
                .paginator
                .references
                .borrow()
                .resolve(&pseudo.content, href);
            match resolved {
                Ok(text) => Some((text, pseudo.paragraph())),
                Err(unresolved) => {
                    let at = origin(self.source, inline_position(inline));
                    self.paginator
                        .warn(unresolved.message(), (!at.is_empty()).then_some(at));
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
    use crate::layout::testing::{book_of, long_prose, registry, section, styled};
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
        runs.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
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
        let (voyage, storm, hunter) = (folio("The Voyage"), folio("The Storm"), folio("The Hunter"));
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

    /// Acceptance: a reference to an id nothing carries warns, naming
    /// the line and column it was written at. It generates nothing,
    /// and the run goes on.
    #[test]
    fn a_reference_to_an_id_nothing_carries_warns_and_generates_nothing() {
        let book = linking_to("#nowhere");
        let (pages, warnings, _) = lay_out(PAGE_REFERENCE, &book);
        assert_eq!(
            warnings,
            [Warning {
                message: "No element has the id `nowhere`. Nothing is generated.".into(),
                origin: Some("one.md:12:5".into()),
            }],
        );
        assert!(page_words(&pages[0]).contains("See elsewhere for the rest."));
        assert!(!page_words(&pages[0]).contains("(page"));
    }

    /// A url that names no id, and `attr(href url)` on an element that
    /// is not a link, generate nothing either, and say why.
    #[test]
    fn a_reference_that_names_no_id_warns() {
        let (_, warnings, _) = lay_out(PAGE_REFERENCE, &linking_to("https://example.com"));
        assert_eq!(
            warnings[0].message,
            "`https://example.com` is not an id in the book. Nothing is generated.",
        );
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

        // A url written into the sheet names its target outright.
        let css = "em::after { content: \" (page \" target-counter(\"#the-voyage\", page) \")\" }";
        let (pages, warnings, _) = lay_out(css, &linking_to("#the-voyage"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(page_words(&pages[0]).contains("See elsewhere (page 1) for the rest."));
    }

    /// Acceptance: laid out twice, a book with references comes out
    /// byte for byte the same, over the same number of pages.
    #[test]
    fn a_book_with_references_lays_out_the_same_twice() {
        let book = cross_referenced();
        let styles = styled(PAGE_REFERENCE, &book);
        let run = || layout_book(&book, &styles, registry(), no_assets());
        let (first, second) = (run(), run());
        assert_eq!(first.pages.len(), second.pages.len());
        assert_eq!(
            crate::wire::encode(&first).expect("the output encodes"),
            crate::wire::encode(&second).expect("the output encodes"),
        );
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

    /// The second pass ships. An element a reference prints the page
    /// of that the second pass set on another page is named.
    #[test]
    fn an_element_the_second_pass_moved_is_named() {
        let folios = |pairs: [(&str, u32); 3]| -> BTreeMap<String, u32> {
            pairs.into_iter().map(|(id, folio)| (id.into(), folio)).collect()
        };
        let found = folios([("moved", 12), ("stayed", 99), ("unprinted", 3)]);
        let landed = folios([("moved", 13), ("stayed", 99), ("unprinted", 4)]);
        let printed = BTreeSet::from(["moved".to_string(), "stayed".to_string()]);
        assert_eq!(
            moved(&found, &landed, &printed),
            [Warning {
                message: "The page printed for `moved` is 12, and the element is on page 13."
                    .into(),
                origin: None,
            }],
        );
    }
}
