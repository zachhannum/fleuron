//! Where the nodes of a whole novel were read from.
//!
//! A node names the bytes of the source it was read from, and a byte
//! of that source names the node. What the tests here hold that to
//! over a book: the spans tile the manuscript, so one byte is one
//! node; a node reads back as the text it was read from; and a run on
//! a page, taken to the manuscript and back, reaches the page it was
//! painted on.

use std::collections::BTreeMap;

use fleuron::content::{Block, Book, Inline, NodeId, SourceSpan, inline_span};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron_fixtures::{Corpus, registry, styles};

/// One node of the tree: what it was read from, and what it holds.
struct Node {
    id: NodeId,
    span: SourceSpan,
    /// The text it was read as, for a node that holds text.
    text: Option<String>,
    /// Whether nothing under it was read from a narrower stretch.
    leaf: bool,
}

/// Every node of a book, parents before the children whose spans they
/// hold.
fn nodes(book: &Book) -> Vec<Node> {
    fn walk_inlines(inlines: &[Inline], out: &mut Vec<Node>) {
        for inline in inlines {
            let (text, children) = match inline {
                Inline::Text { value, .. } | Inline::Code { value, .. } => {
                    (Some(value.clone()), None)
                }
                Inline::Emphasis { children, .. }
                | Inline::Strong { children, .. }
                | Inline::Link { children, .. } => (None, Some(children)),
            };
            out.push(Node {
                id: fleuron::content::inline_id(inline),
                span: inline_span(inline).expect("a parsed inline was read from somewhere"),
                text,
                leaf: children.is_none(),
            });
            if let Some(children) = children {
                walk_inlines(children, out);
            }
        }
    }

    fn walk_blocks(blocks: &[Block], out: &mut Vec<Node>) {
        for block in blocks {
            let held: Option<&[Inline]> = match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => Some(inlines),
                _ => None,
            };
            let quoted = match block {
                Block::Blockquote { blocks, .. } => Some(blocks),
                _ => None,
            };
            out.push(Node {
                id: fleuron::content::block_id(block),
                span: fleuron::content::block_span(block)
                    .expect("a parsed block was read from somewhere"),
                text: None,
                leaf: held.is_none() && quoted.is_none(),
            });
            if let Some(held) = held {
                walk_inlines(held, out);
            }
            if let Some(quoted) = quoted {
                walk_blocks(quoted, out);
            }
        }
    }

    let mut out = Vec::new();
    for section in &book.sections {
        out.push(Node {
            id: section.id,
            span: section
                .span
                .expect("a parsed section was read from somewhere"),
            text: None,
            leaf: false,
        });
        walk_blocks(&section.blocks, &mut out);
    }
    out
}

/// Whether the bytes a run was read from read back as its text. They
/// do unless markdown spelled a character some other way: an entity,
/// or a line the author wrapped, which is a space in the tree and a
/// newline in the file.
fn reads_back(read: &str, text: &str) -> bool {
    read == text
        || (text == " " && read.trim().is_empty())
        || (read.starts_with('&') && read.ends_with(';'))
}

/// Acceptance: over both books, every byte of the source falls in at
/// most one node, every node is held by the one above it, and every
/// node that holds text was read from bytes that read back as that
/// text.
#[test]
fn every_byte_of_the_corpus_falls_in_one_node() {
    for corpus in Corpus::ALL {
        let book = corpus.book();
        let source = corpus.markdown();
        let nodes = nodes(&book);
        assert!(
            nodes.len() > 1_000,
            "{}: {} nodes",
            corpus.slug(),
            nodes.len()
        );

        for node in &nodes {
            assert!(
                node.span.start <= node.span.end && node.span.end as usize <= source.len(),
                "{}: node {} covers {:?}, which is not a stretch of the source",
                corpus.slug(),
                node.id.get(),
                node.span,
            );
            let Some(text) = &node.text else { continue };
            let read = &source[node.span.start as usize..node.span.end as usize];
            assert!(
                reads_back(read, text),
                "{}: node {} was read from {read:?} and holds {text:?}",
                corpus.slug(),
                node.id.get(),
            );
        }

        // The nodes nothing narrower sits inside are what a byte
        // lands on, so it is those that have to tile the file.
        let mut leaves: Vec<&Node> = nodes.iter().filter(|node| node.leaf).collect();
        leaves.sort_by_key(|node| node.span.start);
        for pair in leaves.windows(2) {
            let (first, next) = (pair[0].span, pair[1].span);
            assert!(
                first.end <= next.start,
                "{}: nodes {} and {} were both read from byte {}",
                corpus.slug(),
                pair[0].id.get(),
                pair[1].id.get(),
                next.start,
            );
        }

        // And what holds a node was read from a stretch that holds
        // its own, so the innermost answer is the deepest one.
        let by_id: BTreeMap<u32, SourceSpan> = nodes
            .iter()
            .map(|node| (node.id.get(), node.span))
            .collect();
        for node in &nodes {
            for byte in [node.span.start, node.span.end.saturating_sub(1)] {
                let answer = book
                    .node_at(corpus.source(), byte)
                    .expect("a byte a node was read from is a byte the book answers for");
                let answer = by_id[&answer.get()];
                assert!(
                    answer.start >= node.span.start && answer.end <= node.span.end,
                    "{}: byte {byte} of node {} answered with {answer:?}",
                    corpus.slug(),
                    node.id.get(),
                );
            }
        }
    }
}

/// Which pages each node was painted on, in reading order.
fn painted(pages: &[Page]) -> BTreeMap<u32, (NodeId, Vec<usize>)> {
    let mut out: BTreeMap<u32, (NodeId, Vec<usize>)> = BTreeMap::new();
    for (index, page) in pages.iter().enumerate() {
        for item in &page.items {
            if let DrawItem::Text {
                origin: Some(origin),
                ..
            } = item
            {
                let (_, pages) = out
                    .entry(origin.node.get())
                    .or_insert((origin.node, Vec::new()));
                if pages.last() != Some(&index) {
                    pages.push(index);
                }
            }
        }
    }
    out
}

/// Acceptance: every run of the novel names a node that says where in
/// the manuscript it was written, and that stretch of the manuscript
/// names the node again, which reaches the page the run was on.
#[test]
fn a_run_taken_to_the_manuscript_and_back_reaches_its_page() {
    let book = Corpus::GATE.book();
    let styles = styles(&book);
    let output = layout_book(&book, &styles, registry(), &Assets::none());
    let painted = painted(&output.pages);
    assert!(painted.len() > 1_000, "{} nodes painted", painted.len());

    for (id, (node, pages)) in &painted {
        let (source, span) = book
            .source_of(*node)
            .unwrap_or_else(|| panic!("node {id} was painted and says nothing about its source"));
        assert_eq!(source, Corpus::GATE.source());
        assert_eq!(
            book.node_at(source, span.start),
            Some(*node),
            "node {id} was read from {span:?}, which answers with another node",
        );
        assert!(!pages.is_empty(), "node {id} reaches no page");
    }
}

/// Acceptance: a cursor in a paragraph the fragmenter broke over a
/// page turn answers with the node whose runs are on both pages.
#[test]
fn a_cursor_in_a_broken_paragraph_answers_with_the_node_on_both_pages() {
    let book = Corpus::GATE.book();
    let styles = styles(&book);
    let output = layout_book(&book, &styles, registry(), &Assets::none());
    let painted = painted(&output.pages);

    let broken: Vec<(u32, NodeId, &Vec<usize>)> = painted
        .iter()
        .filter(|(_, (_, pages))| pages.len() > 1)
        .map(|(id, (node, pages))| (*id, *node, pages))
        .collect();
    assert!(
        broken.len() > 100,
        "only {} paragraphs of the novel are broken over a page turn",
        broken.len(),
    );

    for (id, node, pages) in broken {
        let (source, span) = book
            .source_of(node)
            .expect("a painted node says where it was read");
        let middle = midpoint(Corpus::GATE.markdown(), span);
        assert_eq!(
            book.node_at(source, middle),
            Some(node),
            "byte {middle} is inside node {id} and answered with another",
        );
        assert!(
            pages.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "node {id} is painted on {pages:?}, which is not a page turn",
        );
    }
}

/// A byte in the middle of one stretch of the source, on a character
/// boundary.
fn midpoint(source: &str, span: SourceSpan) -> u32 {
    let mut at = (span.start + span.end) / 2;
    while !source.is_char_boundary(at as usize) {
        at += 1;
    }
    at
}
