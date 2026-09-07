//! The source ranges over a whole novel.
//!
//! A run says which node it was shaped from and which bytes of that
//! node it stands for. Over a book, the runs that name one node have
//! to give the node back: a preview that scrolls to the sentence
//! somebody is typing is only as good as that.

use std::collections::BTreeMap;
use std::ops::Range;

use fleuron::content::{Block, Inline};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron_fixtures::{Corpus, registry, styles};

/// Every text node of a book, by id.
fn nodes(book: &fleuron::content::Book) -> BTreeMap<u32, String> {
    fn walk_inlines(inlines: &[Inline], out: &mut BTreeMap<u32, String>) {
        for inline in inlines {
            match inline {
                Inline::Text { id, value, .. } | Inline::Code { id, value, .. } => {
                    out.insert(id.get(), value.clone());
                }
                Inline::Emphasis { children, .. }
                | Inline::Strong { children, .. }
                | Inline::Link { children, .. } => walk_inlines(children, out),
            }
        }
    }

    fn walk_blocks(blocks: &[Block], out: &mut BTreeMap<u32, String>) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    walk_inlines(inlines, out)
                }
                Block::Blockquote { blocks, .. } => walk_blocks(blocks, out),
                Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }

    let mut out = BTreeMap::new();
    for section in &book.sections {
        walk_blocks(&section.blocks, &mut out);
    }
    out
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

/// Acceptance: over the gate novel, the runs that name a node tile it
/// — no overlap, no gap — and the bytes they cover, read back off the
/// node, are the node's own text. A node no run names is a space a
/// line break swallowed, and nothing else.
#[test]
fn every_node_of_the_novel_is_given_back_by_the_runs_that_name_it() {
    let book = Corpus::GATE.book();
    let styles = styles(&book);
    let output = layout_book(&book, &styles, registry(), &Assets::none());
    let nodes = nodes(&book);
    let painted = painted(&output.pages);

    for (id, ranges) in &painted {
        let text = nodes
            .get(id)
            .unwrap_or_else(|| panic!("node {id} is prose"));
        let mut read = String::new();
        let mut at = 0u32;
        for range in ranges {
            assert_eq!(
                range.start, at,
                "node {id} is covered as {ranges:?}, which does not run on from {at}",
            );
            read.push_str(&text[range.start as usize..range.end as usize]);
            at = range.end;
        }
        assert_eq!(&read, text, "node {id} reads back as something else");
    }

    let unpainted: Vec<&String> = nodes
        .iter()
        .filter(|(id, _)| !painted.contains_key(id))
        .map(|(_, text)| text)
        .collect();
    assert!(
        unpainted.iter().all(|text| text.trim().is_empty()),
        "{} nodes with prose in them reached no run: {:?}",
        unpainted.iter().filter(|t| !t.trim().is_empty()).count(),
        unpainted
            .iter()
            .filter(|t| !t.trim().is_empty())
            .take(4)
            .collect::<Vec<_>>(),
    );
}
