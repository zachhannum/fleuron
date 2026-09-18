//! A span: the inline the sheet names and the vocabulary gives no
//! meaning of its own.
//!
//! One manuscript, a chapter opening written as a number over a
//! title, set under sheets that reach the two runs separately.

use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Source, StyleTree, Stylesheets};
use fleuron_markdown::Options;

/// A page wide enough for the heading and long enough for a second
/// page of prose, so a running head has somewhere to print.
const PAGE: &str = "@page { size: 400pt 220pt; margin: 20pt }";

const OPENING: &str = "\
# [Chapter One]{.number} [The Road]{.title}

He arrived on a Tuesday, and nobody met him at the station.
";

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn read(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "chapter-01.md", &Options::default());
    assert!(warnings.is_empty(), "{warnings:?}");
    fleuron_markdown::assemble(Default::default(), sections)
}

fn styles(book: &Book, css: &str) -> StyleTree {
    let styles: StyleTree =
        Stylesheets::parse(&[Source::author("span.css", css)]).compile(book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    styles
}

fn pages(book: &Book, css: &str) -> Vec<Page> {
    Paginator::new(registry(), &styles(book, css)).paginate(book)
}

/// Every run of one page: its text and the em size it was set at.
fn runs(page: &Page) -> Vec<(String, f32)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text { text, size, .. } => Some((text.clone(), *size)),
            _ => None,
        })
        .collect()
}

/// The em size every run holding `words` was set at.
fn sizes(page: &Page, words: &str) -> Vec<f32> {
    runs(page)
        .into_iter()
        .filter(|(text, _)| text.contains(words))
        .map(|(_, size)| size)
        .collect()
}

/// Acceptance: a heading written as a number over a title sets the
/// two runs at the sizes the sheet gives them, inside the one
/// heading.
#[test]
fn a_heading_sets_two_named_runs_at_their_own_sizes() {
    let book = read(OPENING);
    let css = format!(
        "{PAGE}
         h1 {{ font-size: 18pt }}
         .number {{ font-size: 24pt }}
         .title {{ font-size: 12pt }}"
    );
    let page = &pages(&book, &css)[0];
    assert_eq!(sizes(page, "Chapter One"), [24.0]);
    assert_eq!(sizes(page, "The Road"), [12.0]);
}

/// Acceptance: `.number { font-size: 24pt }` reaches that run and no
/// other text in the heading.
#[test]
fn a_class_on_a_span_reaches_that_run_and_no_other() {
    let book = read("# [Chapter One]{.number} The Road\n\nHe arrived on a Tuesday.\n");
    let css = format!(
        "{PAGE}
         p {{ font-size: 10pt }}
         h1 {{ font-size: 18pt }}
         .number {{ font-size: 24pt }}"
    );
    let page = &pages(&book, &css)[0];
    assert_eq!(sizes(page, "Chapter One"), [24.0]);
    assert_eq!(sizes(page, "The Road"), [18.0]);
    assert_eq!(
        sizes(page, "Tuesday"),
        [10.0],
        "the prose keeps its own size"
    );
}

/// Acceptance: a span inherits from the element around it. The sheet
/// names nothing on the span, and the heading's colour, size and
/// spacing reach it.
#[test]
fn a_span_inherits_from_the_element_around_it() {
    let book = read(OPENING);
    let css = format!("{PAGE}\n h1 {{ font-size: 21pt; letter-spacing: 2pt }}");
    let page = &pages(&book, &css)[0];
    assert_eq!(sizes(page, "Chapter One"), [21.0]);
    assert_eq!(sizes(page, "The Road"), [21.0]);

    // An inherited property reaches the span the same way: the
    // heading's tracking widens the run inside it.
    let tight = format!("{PAGE}\n h1 {{ font-size: 21pt }}");
    let narrow = &pages(&book, &tight)[0];
    let width = |page: &Page| {
        page.items
            .iter()
            .find_map(|item| match item {
                DrawItem::Text {
                    x, text, glyphs, ..
                } if text.contains("Chapter One") => {
                    Some(glyphs.last().expect("the run has glyphs").x - x)
                }
                _ => None,
            })
            .expect("the run is drawn")
    };
    assert!(
        width(page) > width(narrow),
        "the tracking did not reach the span",
    );
}

/// Acceptance: `h2 span:last-child { string-set: chapter-title
/// content(text) }` puts that run in the running head, and the run
/// before it stays out of it.
#[test]
fn a_string_set_on_a_span_reaches_the_running_head() {
    let book = read(&format!(
        "## [Chapter One]{{.number}} [The Road]{{.title}}\n\n{}\n",
        "he arrived on a tuesday and nobody met him at the station ".repeat(30),
    ));
    let css = format!(
        "{PAGE}
         h2 span:last-child {{ string-set: chapter-title content(text) }}
         @page {{ @top-center {{ content: string(chapter-title) }} }}"
    );
    let pages = pages(&book, &css);
    assert!(pages.len() > 1, "the chapter fits on one page");
    let printed: Vec<String> = runs(&pages[1])
        .into_iter()
        .map(|(text, _)| text)
        .filter(|text| text.contains("Road") || text.contains("Chapter"))
        .collect();
    assert_eq!(printed, ["The Road"]);
}

/// A span holds a span, and the cascade reaches both: the inner run
/// takes its own size and the rest of the outer run keeps its.
#[test]
fn the_cascade_walks_through_a_span() {
    let book = read("# C\n\nA [named [inner]{.deep} run]{.outer} and prose.\n");
    let css = format!(
        "{PAGE}
         p {{ font-size: 10pt }}
         .outer {{ font-size: 15pt }}
         .deep {{ font-size: 9pt }}"
    );
    let page = &pages(&book, &css)[0];
    assert_eq!(sizes(page, "named"), [15.0]);
    assert_eq!(sizes(page, "inner"), [9.0]);
    assert_eq!(sizes(page, "run"), [15.0]);
    assert_eq!(sizes(page, "and prose"), [10.0]);
}
