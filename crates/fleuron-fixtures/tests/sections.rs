//! The page-to-section mapping over a whole novel.
//!
//! A chapter's opening page is found by what is painted on it, the
//! heading at the size the sheet computes for one, so the mapping is
//! checked against something other than itself.

use fleuron::content::{Block, NodeId};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron_fixtures::{Corpus, registry, styles};

/// The size the sheet sets a section's opening heading at.
fn heading_size(book: &fleuron::content::Book, styles: &fleuron::style::StyleTree) -> f32 {
    let heading = book
        .sections
        .iter()
        .find_map(|section| match section.blocks.first() {
            Some(Block::Heading { id, .. }) => Some(*id),
            _ => None,
        })
        .expect("every chapter of the corpus opens on a heading");
    styles.style(heading).font_size
}

/// True when the page's first paint op is a chapter heading.
fn opens_a_chapter(page: &Page, size: f32) -> bool {
    matches!(page.items.first(), Some(DrawItem::Text { size: s, .. }) if *s == size)
}

/// Every chapter of the gate novel opens on a page that names it, and
/// the chapters run across the book in the order they were written.
#[test]
fn every_chapter_opens_on_a_page_that_names_it() {
    let book = Corpus::GATE.book();
    let styles = styles(&book);
    let output = layout_book(&book, &styles, registry(), &Assets::none());
    let size = heading_size(&book, &styles);
    let ids: Vec<NodeId> = book.sections.iter().map(|section| section.id).collect();

    let mut read: Vec<NodeId> = Vec::new();
    for page in &output.pages {
        for id in &page.sections {
            if read.last() != Some(id) {
                read.push(*id);
            }
        }
    }
    assert_eq!(read, ids, "the pages name the chapters out of order");

    for (chapter, id) in ids.iter().enumerate() {
        let first = output
            .pages
            .iter()
            .position(|page| page.sections.contains(id))
            .expect("every chapter reaches a page");
        assert!(
            opens_a_chapter(&output.pages[first], size),
            "chapter {chapter} first appears on page {} of {}, which opens no chapter",
            first + 1,
            output.pages.len(),
        );
        assert_eq!(
            output.pages[first].sections.first(),
            Some(id),
            "the page chapter {chapter} opens on names another section first",
        );
    }

    // The novel squares its chapters onto rectos, so it has blank
    // leaves, and a blank leaf has nobody's content on it.
    let blanks = output
        .pages
        .iter()
        .filter(|page| page.items.is_empty())
        .count();
    assert!(blanks > 0, "no blank leaf in {} pages", output.pages.len());
    for page in &output.pages {
        assert_eq!(
            page.items.is_empty(),
            page.sections.is_empty(),
            "page {} paints {} items and names {:?}",
            page.number,
            page.items.len(),
            page.sections,
        );
    }
}
