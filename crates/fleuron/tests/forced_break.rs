//! Page and column breaks written in a manuscript, read by the shipped
//! frontend and laid out under the built-in sheet.
//!
//! It lives here rather than beside the layout code because reading a
//! manuscript means the markdown crate, which depends on this one.

use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::{Source, Stylesheets};
use fleuron_markdown::Options;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn book(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "breaks.md", &Options::default());
    assert!(warnings.is_empty(), "{warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

/// The pages a manuscript lays out to under the built-in sheet and
/// `css`.
fn pages(markdown: &str, css: &str) -> Vec<Page> {
    let book = book(markdown);
    let styles =
        Stylesheets::parse(&[Source::author("breaks.css", css)]).compile(&book, registry());
    assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
    layout_book(&book, &styles, registry(), &Assets::none()).pages
}

/// The page that paints `word`, and where the run that paints it
/// starts on that page.
fn place_of(pages: &[Page], word: &str) -> (usize, f32, f32) {
    pages
        .iter()
        .enumerate()
        .find_map(|(index, page)| {
            page.items.iter().find_map(|item| match item {
                DrawItem::Text { x, y, text, .. } if text.contains(word) => Some((index, *x, *y)),
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("nothing paints {word:?}"))
}

/// The page that paints `word`.
fn page_of(pages: &[Page], word: &str) -> usize {
    place_of(pages, word).0
}

/// The text of the highest run on one page.
fn first_run(page: &Page) -> &str {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text { y, text, .. } => Some((*y, text.as_str())),
            _ => None,
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, text)| text)
        .expect("the page paints text")
}

/// Acceptance: `\pagebreak` alone on a line starts a new page, and the
/// prose after it opens that page.
#[test]
fn a_page_break_starts_a_new_page() {
    let pages = pages("# One\n\nLilliput.\n\n\\pagebreak\n\nBlefuscu.\n", "");
    assert_eq!(pages.len(), 2);
    assert_eq!(page_of(&pages, "Lilliput"), 0);
    assert_eq!(page_of(&pages, "Blefuscu"), 1);
    assert!(
        first_run(&pages[1]).contains("Blefuscu"),
        "the page opens on {:?}",
        first_run(&pages[1])
    );
}

/// Acceptance: `\columnbreak` alone on a line starts the next column
/// on a page of two columns.
#[test]
fn a_column_break_starts_the_next_column() {
    let pages = pages(
        "# One\n\nLilliput.\n\n\\columnbreak\n\nBlefuscu.\n",
        "@page { column-count: 2 }",
    );
    assert_eq!(pages.len(), 1, "a column break does not turn the page");
    let (_, before_x, before_y) = place_of(&pages, "Lilliput");
    let (_, after_x, after_y) = place_of(&pages, "Blefuscu");
    assert!(
        after_x > before_x,
        "the prose after the break is in the second column"
    );
    assert!(
        after_y < before_y,
        "the prose after the break opens the second column"
    );
}

/// Acceptance: a sheet rule on the class an attribute line names
/// changes the break.
#[test]
fn a_rule_on_the_class_of_a_break_changes_it() {
    let manuscript = "# One\n\nLilliput.\n\n{.soft}\n\n\\pagebreak\n\nBlefuscu.\n";
    assert_eq!(pages(manuscript, "").len(), 2);
    assert_eq!(
        pages(manuscript, ".soft { break-after: column }").len(),
        2,
        "on a page of one column, a column break starts a new page"
    );
    assert_eq!(pages(manuscript, ".soft { break-after: auto }").len(), 1);
}

/// Acceptance: `pagebreak { break-after: auto }` in a sheet removes the
/// break.
#[test]
fn a_sheet_can_remove_every_page_break() {
    let pages = pages(
        "# One\n\nLilliput.\n\n\\pagebreak\n\nBlefuscu.\n",
        "pagebreak { break-after: auto }",
    );
    assert_eq!(pages.len(), 1);
}

/// Acceptance: two breaks with nothing between them start one new page,
/// not a blank page.
#[test]
fn two_breaks_in_a_row_are_one_break() {
    let pages = pages(
        "# One\n\nLilliput.\n\n\\pagebreak\n\n\\pagebreak\n\nBlefuscu.\n",
        "",
    );
    assert_eq!(pages.len(), 2);
    assert_eq!(page_of(&pages, "Blefuscu"), 1);
}

/// Acceptance: a page break at the end of a chapter keeps the next
/// chapter on a right-hand page, and adds no blank page to what the
/// chapter opening already asks for.
#[test]
fn a_page_break_at_the_end_of_a_chapter_keeps_the_next_on_the_right() {
    let plain = pages("# One\n\nLilliput.\n\n# Two\n\nBlefuscu.\n", "");
    let broken = pages(
        "# One\n\nLilliput.\n\n\\pagebreak\n\n# Two\n\nBlefuscu.\n",
        "",
    );
    assert_eq!(
        page_of(&plain, "Blefuscu"),
        2,
        "chapter two opens on page 3"
    );
    assert_eq!(broken.len(), plain.len());
    assert_eq!(page_of(&broken, "Blefuscu"), 2);
}

/// Acceptance: a page break before a block with `break-before: recto`
/// keeps that block on a right-hand page.
#[test]
fn a_page_break_before_a_right_hand_block_keeps_it_on_the_right() {
    let pages = pages(
        "# One\n\nLilliput.\n\n\\pagebreak\n\n{.plate}\n\nBlefuscu.\n",
        ".plate { break-before: recto }",
    );
    assert_eq!(page_of(&pages, "Lilliput"), 0);
    assert_eq!(page_of(&pages, "Blefuscu"), 2, "the block opens page 3");
    assert_eq!(pages[2].side, Side::Recto);
}

/// Acceptance: a break in a blockquote or a list item still breaks,
/// the last block of either included.
#[test]
fn a_break_in_a_quote_or_an_item_still_breaks() {
    let pages = pages(
        "# One\n\n> Lilliput.\n>\n> \\pagebreak\n\nBlefuscu.\n\n- Brobdingnag\n\n  \\pagebreak\n- Laputa\n",
        "",
    );
    assert_eq!(page_of(&pages, "Lilliput"), 0);
    assert_eq!(page_of(&pages, "Blefuscu"), 1);
    assert_eq!(page_of(&pages, "Brobdingnag"), 1);
    assert_eq!(page_of(&pages, "Laputa"), 2);
}
