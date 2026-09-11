//! A whole novel whose chapters refer to one another: a reference
//! near the front of the book to a chapter near the back, and one
//! back again.
//!
//! A page number with a digit more or fewer than the placeholder is
//! what a book-scale run has and a short book does not. Each printed
//! number is right only where the second pass left every chapter on
//! the page the first pass found it on.

use fleuron::content::{Block, Book, NodeId, block_id};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};
use fleuron_fixtures::{Corpus, registry};

const PAGE_REFERENCE: &str =
    "a::after { content: \" (page \" target-counter(attr(href url), page) \")\" }";

/// The gate novel with an id on two chapter headings, and a link at
/// the head of each chapter to the other.
fn referring() -> Book {
    let mut markdown = String::new();
    for line in Corpus::GATE.markdown().lines() {
        match line {
            "## Chapter 3" => {
                markdown.push_str("## Chapter 3 {#chapter-3}\n\nSee [chapter 57](#chapter-57).\n")
            }
            "## Chapter 57" => {
                markdown.push_str("## Chapter 57 {#chapter-57}\n\nSee [chapter 3](#chapter-3).\n")
            }
            _ => {
                markdown.push_str(line);
                markdown.push('\n');
            }
        }
    }
    Corpus::GATE.parse(&markdown)
}

/// The heading that carries `id`.
fn heading(book: &Book, id: &str) -> NodeId {
    book.sections
        .iter()
        .flat_map(|section| &section.blocks)
        .find(|block| {
            matches!(block, Block::Heading { attributes, .. } if attributes.id.as_deref() == Some(id))
        })
        .map(block_id)
        .unwrap_or_else(|| panic!("no heading carries `{id}`"))
}

/// Every page number the references print, in reading order.
fn printed(pages: &[Page]) -> Vec<u32> {
    let words: Vec<&str> = pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    words
        .join(" ")
        .split("(page ")
        .skip(1)
        .map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().expect("a number follows every reference")
        })
        .collect()
}

/// Acceptance: a reference on page 300 to a target on page 12 prints
/// 12, and the reverse prints 300. The two numbers have different
/// numbers of digits, and neither pass moves a chapter the other found.
///
/// The single run the CLI makes and the session a preview keeps set
/// the same book byte for byte, over the same number of pages.
#[test]
fn a_novel_refers_across_three_hundred_pages() {
    let mut session = Session::new(registry());
    session.set_content(referring());
    session.set_style(Stylesheets::parse(&[Source::author(
        "references.css",
        PAGE_REFERENCE,
    )]));
    let book = session.book();
    let chapters = [heading(book, "chapter-3"), heading(book, "chapter-57")];
    let folios = session.folios(&chapters);
    let (early, late) = (
        folios[0].expect("chapter 3 is set").first,
        folios[1].expect("chapter 57 is set").first,
    );
    assert!(
        early < 100 && late >= 100,
        "chapter 3 opens on page {early} and chapter 57 on page {late}",
    );

    let output = session.preview();
    assert_eq!(printed(&output.pages), [late, early]);
    assert!(
        !output.warnings.iter().any(|warning| {
            warning.message.contains("page printed") || warning.message.contains("generated")
        }),
        "{:?}",
        output.warnings,
    );
    let pages = output.pages.len();
    let preview = fleuron::wire::encode(output).expect("the preview encodes");
    assert_eq!(session.stages().settle, 1);

    let styles = session.styles().clone();
    let once = layout_book(session.book(), &styles, registry(), &Assets::none());
    assert_eq!(once.pages.len(), pages, "the page count moved");
    assert_eq!(
        fleuron::wire::encode(&once).expect("the run encodes"),
        preview,
        "the single run and the preview set the book differently",
    );
}
