//! Where a section starts: on the next page under the built-in sheet,
//! and on a right-hand page under a sheet that asks for one.
//!
//! The books are manuscripts read by the shipped frontend, so a
//! section here is what a heading makes it.

use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{Page, Side};
use fleuron::style::{Break, Source, Stylesheets};
use fleuron_markdown::Options;

/// A title page, a copyright page and a chapter, each a page long.
const THREE_SECTIONS: &str = "# Travels\n\nLemuel Gulliver.\n\n\
                              # Copyright\n\nPrinted in Lilliput.\n\n\
                              # One\n\nBlefuscu.\n";

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn book(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "sections.md", &Options::default());
    assert!(warnings.is_empty(), "{warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

/// The pages a book lays out to under the built-in sheet and `css`.
fn pages(book: &Book, css: &str) -> Vec<Page> {
    let styles =
        Stylesheets::parse(&[Source::author("sections.css", css)]).compile(book, registry());
    assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
    layout_book(book, &styles, registry(), &Assets::none()).pages
}

/// Acceptance: `user-agent.css` gives `section` a `break-before` of
/// `page`.
#[test]
fn the_built_in_sheet_starts_a_section_on_the_next_page() {
    let book = book(THREE_SECTIONS);
    let styles = fleuron::style::defaults(&book, registry());
    assert_eq!(book.sections.len(), 3);
    for section in &book.sections {
        assert_eq!(styles.style(section.id).break_before, Break::Page);
    }
}

/// Acceptance: a book of three one-page sections with no sheet of its
/// own sets three pages, with no blank page among them.
#[test]
fn three_one_page_sections_are_three_pages() {
    let book = book(THREE_SECTIONS);
    let pages = pages(&book, "");
    assert_eq!(pages.len(), 3);
    for (page, section) in pages.iter().zip(&book.sections) {
        assert!(!page.items.is_empty(), "page {} is blank", page.number);
        assert_eq!(page.sections, [section.id]);
    }
}

/// Acceptance: `section { break-before: recto; }` in a cascaded sheet
/// still leaves a blank left-hand page before a section that would
/// start on one.
#[test]
fn a_sheet_that_asks_for_a_recto_still_gets_the_blank_verso() {
    let book = book(THREE_SECTIONS);
    let pages = pages(&book, "section { break-before: recto; }");
    assert_eq!(pages.len(), 5);
    for (index, page) in pages.iter().enumerate() {
        let blank = index % 2 == 1;
        assert_eq!(page.items.is_empty(), blank, "page {}", page.number);
        assert_eq!(page.side == Side::Verso, blank, "page {}", page.number);
    }
}
