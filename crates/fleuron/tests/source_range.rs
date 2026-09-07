//! Where a laid-out run says it was written.
//!
//! A run names the content node it was shaped from and the bytes of
//! that node it stands for. What the tests here hold it to: the
//! ranges tile the manuscript, a break does not drop the byte it fell
//! on, a glyph reaches the letters it swallowed rather than the whole
//! run, and text nobody wrote names nobody.

use std::collections::BTreeMap;
use std::ops::Range;

use fleuron::content::{Block, Book, HeadingLevel, Inline, NodeId, SourceRange};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Source, StyleTree, Stylesheets};

/// A page small enough that one paragraph fills more than one of
/// them, with a folio in the bottom margin and the chapter's title
/// running along the top.
const CSS: &str = r#"
@page {
  size: 200pt 120pt;
  margin: 24pt;
  @top-center { content: string(chapter); font-size: 8pt }
  @bottom-center { content: counter(page); font-size: 8pt }
}

h1 { font-size: 12pt }
"#;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        position: None,
    }
}

/// A chapter and the blocks under it, ids assigned.
fn book(blocks: Vec<Block>) -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![fleuron::content::Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks,
            position: None,
        }],
    };
    book.assign_node_ids();
    book
}

fn heading(title: &str) -> Block {
    Block::Heading {
        id: NodeId::UNASSIGNED,
        level: HeadingLevel::H1,
        inlines: vec![text(title)],
        position: None,
    }
}

fn paragraph(inlines: Vec<Inline>) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines,
        position: None,
    }
}

fn styles(book: &Book, css: &str) -> StyleTree {
    Stylesheets::parse(&[Source::author("ranges.css", css)]).compile(book, registry())
}

fn pages(book: &Book, css: &str) -> Vec<Page> {
    Paginator::new(registry(), &styles(book, css)).paginate(book)
}

/// Every text node of a book, by id.
fn nodes(book: &Book) -> BTreeMap<u32, String> {
    fn walk(inlines: &[Inline], out: &mut BTreeMap<u32, String>) {
        for inline in inlines {
            match inline {
                Inline::Text { id, value, .. } | Inline::Code { id, value, .. } => {
                    out.insert(id.get(), value.clone());
                }
                Inline::Emphasis { children, .. }
                | Inline::Strong { children, .. }
                | Inline::Link { children, .. } => walk(children, out),
            }
        }
    }
    let mut out = BTreeMap::new();
    for section in &book.sections {
        for block in &section.blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    walk(inlines, &mut out)
                }
                _ => {}
            }
        }
    }
    out
}

/// The id of the node whose text is `value`.
fn node_of(book: &Book, value: &str) -> u32 {
    *nodes(book)
        .iter()
        .find(|(_, text)| text.as_str() == value)
        .expect("the value is a node of its own")
        .0
}

/// The text runs of a page, in paint order.
fn runs(page: &Page) -> Vec<(&str, &Option<SourceRange>)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                text,
                origin: text_origin,
                ..
            } => Some((text.as_str(), text_origin)),
            _ => None,
        })
        .collect()
}

/// What each node was painted as, in reading order. Paint order is
/// not quite that: a drop cap is drawn after the line it is sunk
/// into, and it holds the letter that line starts past.
fn painted(pages: &[Page]) -> BTreeMap<u32, Vec<Range<u32>>> {
    let mut out: BTreeMap<u32, Vec<Range<u32>>> = BTreeMap::new();
    for page in pages {
        for item in &page.items {
            if let DrawItem::Text {
                origin: Some(origin),
                ..
            } = item
            {
                out.entry(origin.node.get())
                    .or_default()
                    .push(origin.range.clone());
            }
        }
    }
    for ranges in out.values_mut() {
        ranges.sort_by_key(|range| range.start);
    }
    out
}

/// Prose long enough to fill more than one of the small pages the
/// sheet sets, in one text node, so a page break falls inside it.
const PROSE: &str = "My father had a small estate in Nottinghamshire; I was the third \
    of five sons. He sent me to Emanuel College in Cambridge at fourteen years old, \
    where I resided three years, and applied myself close to my studies; but the \
    charge of maintaining me, although I had a very scanty allowance, being too \
    great for a narrow fortune, I was bound apprentice to Mr. James Bates, an \
    eminent surgeon in London, with whom I continued four years.";

/// Acceptance: a paragraph broken across a page boundary comes back
/// as ranges that meet exactly where the break fell. The last range
/// on one page ends where the first on the next begins, and together
/// they are the whole paragraph.
#[test]
fn a_paragraph_broken_across_a_page_meets_at_the_break() {
    let book = book(vec![heading("Chapter One"), paragraph(vec![text(PROSE)])]);
    let pages = pages(&book, CSS);
    assert!(pages.len() > 1, "the paragraph fits on one page");

    let node = *nodes(&book)
        .iter()
        .find(|(_, value)| value.as_str() == PROSE)
        .expect("the prose is a node of its own")
        .0;
    let ranges = painted(&pages)
        .remove(&node)
        .expect("the prose reaches the pages");

    let mut at = 0u32;
    for range in &ranges {
        assert_eq!(
            range.start, at,
            "the ranges {ranges:?} leave a gap or overlap"
        );
        at = range.end;
    }
    assert_eq!(
        at as usize,
        PROSE.len(),
        "the ranges stop short of the prose"
    );

    // The break itself: the ranges on either side of the page turn.
    let first = pages
        .iter()
        .position(|page| painted(std::slice::from_ref(page)).contains_key(&node))
        .expect("the prose reaches a page");
    let ends = painted(&pages[first..first + 1])[&node]
        .last()
        .expect("the page paints the prose")
        .end;
    let starts = painted(&pages[first + 1..first + 2])[&node]
        .first()
        .expect("the next page carries the prose on")
        .start;
    assert_eq!(ends, starts, "the page turn dropped or repeated a byte");
}

/// The run that names `node`, on the first page it is painted on.
fn run_of(pages: &[Page], node: u32) -> (&SourceRange, &str, &[u32], &[fleuron::pages::Glyph]) {
    pages
        .iter()
        .flat_map(|page| &page.items)
        .find_map(|item| match item {
            DrawItem::Text {
                origin: Some(origin),
                source,
                source_map,
                glyphs,
                ..
            } if origin.node.get() == node => {
                Some((origin, source.as_str(), &source_map[..], &glyphs[..]))
            }
            _ => None,
        })
        .expect("the node is painted")
}

/// Acceptance: a glyph standing for more than one letter reaches back
/// to the letters it swallowed, not to the run around them; and a
/// letter written as several glyphs is read back once, off the first
/// of them.
#[test]
fn a_glyph_maps_back_to_the_bytes_it_stands_for() {
    // A ligature: `ffi` is one glyph over three bytes of the node.
    let word = "office";
    let tied_book = book(vec![paragraph(vec![text("the "), text(word)])]);
    let tied_pages = pages(&tied_book, CSS);
    let node = node_of(&tied_book, word);
    let (origin, _, _, glyphs) = run_of(&tied_pages, node);
    let tied = glyphs
        .iter()
        .find(|glyph| glyph.range.end - glyph.range.start > 1)
        .expect("`ffi` shapes as one glyph in the bundled face");
    let at = |byte: u32| (origin.range.start + byte) as usize;
    assert_eq!(&word[at(tied.range.start)..at(tied.range.end)], "ffi");
    assert!(
        tied.range.end - tied.range.start < origin.range.end - origin.range.start,
        "the glyph claims the whole run",
    );

    // A decomposed cluster: `ß` is set as `SS`, and the second of the
    // two stands for nothing anybody wrote.
    let word = "straße";
    let raised = book(vec![paragraph(vec![text("die "), text(word)])]);
    let set = pages(
        &raised,
        &format!("{CSS}\np {{ text-transform: uppercase }}"),
    );
    let node = node_of(&raised, word);
    let (origin, source, map, glyphs) = run_of(&set, node);
    assert_eq!(source, word, "the run does not carry what was written");
    let doubled: Vec<(u32, u32)> = glyphs
        .iter()
        .map(|glyph| {
            (
                map[glyph.range.start as usize],
                map[glyph.range.end as usize],
            )
        })
        .filter(|(from, to)| word[*from as usize..*to as usize] == *"ß" || from == to)
        .collect();
    let at = |byte: u32| (origin.range.start + byte) as usize;
    let [(from, to), (empty, same)] = doubled[..] else {
        panic!("`ß` did not set as two glyphs: {doubled:?}");
    };
    assert_eq!(
        &word[at(from)..at(to)],
        "ß",
        "the first S stands for something else"
    );
    assert_eq!(
        empty, same,
        "the second S stands for a stretch of the manuscript"
    );
}

/// Acceptance: the folio and the running head are the engine's own
/// text, so they name no node, while every run of prose on the same
/// page names one.
#[test]
fn synthesized_text_names_no_node() {
    let book = book(vec![heading("Chapter One"), paragraph(vec![text(PROSE)])]);
    let pages = pages(&book, CSS);
    let page = pages.last().expect("the book has pages");

    let furniture: Vec<&str> = runs(page)
        .into_iter()
        .filter(|(_, origin)| origin.is_none())
        .map(|(text, _)| text)
        .collect();
    assert!(
        furniture.contains(&page.number.to_string().as_str()),
        "the folio names a node: {furniture:?}",
    );
    assert!(
        furniture.contains(&"Chapter One"),
        "the running head names a node: {furniture:?}",
    );
    assert!(
        runs(page)
            .into_iter()
            .any(|(text, origin)| origin.is_some() && text.len() > 1),
        "no prose on the page names its node",
    );
}

/// A drop cap is set from a letter the prose beside it no longer
/// carries, so the cap names it: the paragraph's first node is
/// covered from its first byte all the same.
#[test]
fn a_drop_cap_names_the_letter_it_took() {
    let book = book(vec![paragraph(vec![text(PROSE)])]);
    let pages = pages(
        &book,
        &format!("{CSS}\np::first-letter {{ initial-letter: 3 }}"),
    );
    let node = node_of(&book, PROSE);
    let ranges = painted(&pages).remove(&node).expect("the prose is painted");

    assert_eq!(ranges[0], 0..1, "the cap does not name the letter it took");
    let mut at = 0u32;
    for range in &ranges {
        assert_eq!(
            range.start, at,
            "the ranges {ranges:?} leave a gap or overlap"
        );
        at = range.end;
    }
    assert_eq!(
        at as usize,
        PROSE.len(),
        "the ranges stop short of the prose"
    );
}

/// Acceptance: laying the same book out twice gives the same ranges,
/// so what a host reads off a run does not depend on the run.
#[test]
fn the_ranges_are_the_same_on_every_run() {
    let book = book(vec![heading("Chapter One"), paragraph(vec![text(PROSE)])]);
    assert_eq!(painted(&pages(&book, CSS)), painted(&pages(&book, CSS)));
}
