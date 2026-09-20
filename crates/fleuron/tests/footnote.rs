//! Footnotes through the whole engine: markdown in, pages out.
//!
//! Each test reads a manuscript the way the frontend reads one, lays
//! it out under the built-in sheet and a few rules of its own, and
//! reads the notes back off the display structure.
//!
//! Every paragraph and every note is written with a word of its own,
//! so a page is asked what it holds by asking for that word.

use fleuron::LayoutOutput;
use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, FontSource, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::{Paginator, layout_book};
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Source, StyleTree, Stylesheets};
use fleuron_markdown::Options;
use proptest::prelude::*;

/// The book the whole pipeline is run over, notes and all.
const FIXTURE: &str = include_str!("../../../fixtures/gulliver-excerpt.md");

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A face with no superior figures of its own, for the diagnostic
/// that names one.
const PLAIN_FACE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/fonts/IMFellEnglishSC-Regular.ttf"
);

fn plain_registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = bundled_registry().expect("bundled font parses");
        let bytes = std::fs::read(PLAIN_FACE).expect("the second face is checked in");
        registry
            .add(FontSource::from_bytes(bytes).expect("the second face parses"))
            .expect("the second face registers");
        registry
    })
}

/// A manuscript read the way the frontend reads one.
fn read(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "notes.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn styled(book: &Book, css: &str) -> StyleTree {
    let styles = Stylesheets::parse(&[Source::author("notes.css", css)]).compile(book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings()
    );
    styles
}

fn lay_out(markdown: &str, css: &str) -> LayoutOutput {
    let book = read(markdown);
    let styles = styled(&book, css);
    layout_book(&book, &styles, registry(), &Assets::none())
}

/// Every word one page holds. A line of a page is painted in as many
/// runs as its styling asks for, so the words are what a page is
/// asked about rather than the runs.
fn words(page: &Page) -> Vec<String> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .flat_map(str::split_whitespace)
        .map(str::to_string)
        .collect()
}

/// The pages that hold one word.
fn holding(pages: &[Page], word: &str) -> Vec<u32> {
    pages
        .iter()
        .filter(|page| words(page).iter().any(|held| held == word))
        .map(|page| page.number)
        .collect()
}

/// The marks the notes of a book are set beside, page by page. A
/// mark is the one run of a note that ends in a space: `1. `, as
/// CSS writes the marker of a list item.
fn marks(pages: &[Page]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|page| {
            page.items.iter().filter_map(|item| match item {
                DrawItem::Text { text, .. } if is_mark(text) => Some(text.trim().to_string()),
                _ => None,
            })
        })
        .collect()
}

fn is_mark(text: &str) -> bool {
    let mark = text.trim_end();
    text.ends_with(' ')
        && mark.len() <= 3
        && mark.ends_with('.')
        && mark.starts_with(|first: char| first.is_alphanumeric())
}

/// The runs of a book that ask the face for one feature.
fn asking_for(pages: &[Page], tag: &[u8; 4]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|page| {
            page.items.iter().filter_map(|item| match item {
                DrawItem::Text { text, features, .. }
                    if features
                        .settings()
                        .iter()
                        .any(|setting| setting.tag == *tag) =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
        })
        .collect()
}

/// A manuscript of `count` paragraphs, each one word of its own, with
/// `note` written into the paragraph at `at`.
fn manuscript(count: usize, at: usize, note: &str) -> String {
    let mut out = String::new();
    for index in 1..=count {
        out.push_str(&format!("Line{index} of the manuscript runs along"));
        if index == at {
            out.push_str("[^a]");
        }
        out.push_str(".\n\n");
    }
    out.push_str(&format!("[^a]: {note}\n"));
    out
}

/// A note of `count` words, each word its own.
fn note_of(count: usize) -> String {
    (1..=count)
        .map(|index| format!("note{index} "))
        .collect::<String>()
}

/// Acceptance: a reference pushed to the next page takes its note
/// with it.
///
/// The area comes off the content box before a line is placed, so the
/// line a reference is on moves when the note under it no longer
/// leaves room for it. Wherever the reference lands, the note opens
/// at the foot of that page.
#[test]
fn a_reference_pushed_to_the_next_page_takes_its_note_with_it() {
    let note = note_of(40);
    for at in 20..=45 {
        let pages = lay_out(&manuscript(60, at, &note), "").pages;
        let reference = holding(&pages, &format!("Line{at}"));
        let opened = holding(&pages, "note1");
        assert_eq!(reference.len(), 1, "the reference is on one page");
        assert_eq!(
            opened, reference,
            "the note opened on another page than its reference, with the reference at {at}",
        );
    }
}

/// Acceptance: the line the reference is on is the line that moves.
///
/// The same manuscript with a note of one word keeps that line on the
/// page before, so the move is the area's doing and not the
/// manuscript's.
#[test]
fn the_area_moves_the_line_its_reference_is_on() {
    let note = note_of(40);
    let moved = (20..=45).find(|at| {
        let line = format!("Line{at}");
        let with = lay_out(&manuscript(60, *at, &note), "").pages;
        let without = lay_out(&manuscript(60, *at, "short"), "").pages;
        holding(&with, &line) != holding(&without, &line)
    });
    assert!(
        moved.is_some(),
        "no reference moved: the area never shortened the content box",
    );
}

/// Acceptance: a note longer than the page it starts on continues on
/// the next one, and the continuation carries no reference of its
/// own.
#[test]
fn a_note_longer_than_its_page_continues() {
    let pages = lay_out(&manuscript(20, 3, &note_of(1500)), "").pages;
    let opened = holding(&pages, "note1");
    let ended = holding(&pages, "note1500");
    assert_eq!(opened.len(), 1, "the note opens once");
    assert_eq!(ended.len(), 1, "the note ends once");
    assert!(
        ended[0] > opened[0],
        "the note ended on the page it opened on",
    );
    assert_eq!(marks(&pages), ["1."], "the continuation carries a mark");
}

/// Acceptance: the numbering settles, in a bounded number of passes.
///
/// A note numbered by the page it is set on takes its number from the
/// pages of the pass before, and a number of another width can move
/// the line its reference is on. The book is laid out again until the
/// numbering stops changing.
#[test]
fn the_settle_terminates() {
    let mut markdown = String::new();
    for index in 1..=80 {
        markdown.push_str(&format!(
            "Line{index} of the manuscript runs along[^{index}].\n\n"
        ));
    }
    for index in 1..=80 {
        markdown.push_str(&format!("[^{index}]: note{index} at the foot.\n\n"));
    }
    let book = read(&markdown);
    let styles = styled(&book, "notes { counter-reset: note }");
    let paginator = Paginator::new(registry(), &styles);
    let pages = paginator.paginate(&book);
    assert!(pages.len() > 2, "a book of more than one page");
    assert!(
        paginator.settles() < 4,
        "the numbering took {} passes and did not settle",
        paginator.settles(),
    );
    let again = Paginator::new(registry(), &styles).paginate(&book);
    assert_eq!(pages.len(), again.len(), "the settled page count is stable");
}

/// Acceptance: the numbering settles on the fixture book, which is
/// the book the whole pipeline is run over.
#[test]
fn the_settle_terminates_on_the_fixture_book() {
    let book = read(FIXTURE);
    let styles = styled(&book, "notes { counter-reset: note }");
    let paginator = Paginator::new(registry(), &styles);
    let pages = paginator.paginate(&book);
    assert!(pages.len() > 2, "a book of more than one page");
    assert!(
        paginator.settles() < 4,
        "the numbering took {} passes and did not settle",
        paginator.settles(),
    );
    assert!(
        marks(&pages).iter().any(|mark| mark == "1."),
        "the fixture book set no note",
    );
}

/// A note written in a table cell is set at the foot of the page the
/// row lands on.
#[test]
fn a_note_in_a_table_cell_reaches_the_foot_of_the_page() {
    let markdown = "| Ship | Sailed |\n|---|---|\n\
                    | The Hesper[^a] | 3 March |\n\n\
                    [^a]: note1 of the ship.\n";
    let pages = lay_out(markdown, "").pages;
    assert_eq!(holding(&pages, "note1"), [1], "the note left the book");
    assert_eq!(marks(&pages), ["1."]);
}

/// The first note of every page is numbered 1 where the area
/// restarts the counter.
#[test]
fn the_area_numbers_the_notes_of_each_page_from_one() {
    let mut markdown = String::new();
    for index in 1..=60 {
        markdown.push_str(&format!(
            "Line{index} of the manuscript runs along[^{index}].\n\n"
        ));
    }
    for index in 1..=60 {
        markdown.push_str(&format!("[^{index}]: note{index} at the foot.\n\n"));
    }
    let pages = lay_out(&markdown, "notes { counter-reset: note }").pages;
    assert!(pages.len() > 2, "a book of more than one page");
    for page in &pages {
        let marks = marks(std::slice::from_ref(page));
        assert_eq!(
            marks.first().map(String::as_str),
            Some("1."),
            "page {} opens its area at {:?}",
            page.number,
            marks.first(),
        );
    }
}

/// The counter runs through the book, restarts at every chapter where
/// a section asks it to, and is spelled as `list-style-type` says.
#[test]
fn the_cascade_says_how_the_notes_are_numbered() {
    let chapters = "# One\n\nA line[^a] and another[^b].\n\n[^a]: First.\n\n[^b]: Second.\n\n\
                    # Two\n\nA third[^c].\n\n[^c]: Third.\n";
    let numbered = |css: &str| marks(&lay_out(chapters, css).pages);
    assert_eq!(
        numbered(""),
        ["1.", "2.", "1."],
        "the built-in sheet restarts the counter at every chapter",
    );
    assert_eq!(
        numbered("section { counter-reset: none }"),
        ["1.", "2.", "3."],
        "the counter runs through the book",
    );
    assert_eq!(
        numbered("note { list-style-type: lower-alpha }"),
        ["a.", "b.", "a."],
        "the marks are spelled as list-style-type asks",
    );
}

/// The area is a box of its own: a sheet gives it a rule and space,
/// and the notes stay at the foot of the page inside it.
#[test]
fn the_sheet_styles_the_area() {
    let markdown = manuscript(4, 2, "A note at the foot of the page.");
    let plain = lay_out(&markdown, "").pages;
    let ruled = lay_out(
        &markdown,
        "notes { border-top: 4pt solid; padding-top: 12pt }",
    )
    .pages;
    let rule = |pages: &[Page]| -> (f32, f32) {
        pages[0]
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                DrawItem::Rect { y, h, .. } => Some((*y, *h)),
                _ => None,
            })
            .expect("the area paints a rule")
    };
    assert!(rule(&ruled).1 > rule(&plain).1, "the rule did not thicken");
    assert!(
        rule(&ruled).0 < rule(&plain).0,
        "the taller area did not start higher up the page",
    );
    let foot = |pages: &[Page]| -> f32 {
        pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                DrawItem::Text { y, text, .. } if text.starts_with("A note") => Some(*y),
                _ => None,
            })
            .next_back()
            .expect("the note is set")
    };
    assert!(
        (foot(&plain) - foot(&ruled)).abs() < 1.0,
        "the notes left the foot of the page",
    );
}

/// The padding of a note is the indent its number hangs in at the
/// foot of the page. The reference in the line takes none of it, and
/// stands against the word it was written after.
#[test]
fn the_padding_of_a_note_reaches_the_foot_of_the_page_alone() {
    let markdown = manuscript(4, 2, "note1 at the foot.");
    let run = |css: &str, wanted: &str| -> f32 {
        lay_out(&markdown, css)
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .find_map(|item| match item {
                DrawItem::Text { x, text, .. } if text.trim() == wanted => Some(*x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the book says {wanted:?}"))
    };
    let wide = "note { padding-left: 4em }";
    assert_eq!(
        run("", "1"),
        run(wide, "1"),
        "the padding of the note moved its reference",
    );
    assert!(
        run(wide, "1.") > run("", "1."),
        "the padding of the note left the indent its number hangs in",
    );
}

/// A note is set under the text of the page its reference is on.
#[test]
fn the_note_is_set_under_the_text_of_the_page() {
    let pages = lay_out(&manuscript(4, 2, "note1 at the foot."), "").pages;
    assert_eq!(pages.len(), 1);
    assert_eq!(holding(&pages, "note1"), [1], "the note is not on the page");
    let baseline = |word: &str| -> f32 {
        pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                DrawItem::Text { y, text, .. } if text.contains(word) => Some(*y),
                _ => None,
            })
            .next_back()
            .unwrap_or_else(|| panic!("the page says {word}"))
    };
    assert!(
        baseline("Line4") < baseline("note1"),
        "the note is above the last line of the page",
    );
}

/// Acceptance: the reference of a note is set in the superior
/// figures of a face that carries them, and the figures in the prose
/// of the note are the ordinary ones.
///
/// `font-feature-settings` inherits, so the engine asks for the
/// feature on the reference rather than leaving it to a rule on the
/// note.
#[test]
fn the_reference_of_a_note_is_set_in_superior_figures() {
    let pages = lay_out(&manuscript(4, 2, "Printed in 1805 at the foot."), "").pages;
    assert_eq!(
        asking_for(&pages, b"sups"),
        ["1"],
        "the reference of the note is the one run set in superiors",
    );
    let ordinary: Vec<String> = pages[0]
        .items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text { text, features, .. }
                if text.contains("1805") && features.settings().is_empty() =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        ordinary.len(),
        1,
        "the figures in the prose of the note ask for a feature: {:#?}",
        pages[0].items,
    );
}

/// Acceptance: a face that carries no superior figures sets the
/// reference on the baseline, and is named once however many notes
/// the book has.
#[test]
fn a_face_without_superior_figures_sets_the_reference_on_the_baseline() {
    let markdown = format!(
        "{}{}",
        manuscript(4, 2, "note1 at the foot."),
        "\nAnother line[^b] of the manuscript.\n\n[^b]: note2 at the foot.\n"
    );
    let book = read(&markdown);
    let css = "book { font-family: \"IM Fell English SC\" }";
    let styles =
        Stylesheets::parse(&[Source::author("notes.css", css)]).compile(&book, plain_registry());
    let output = layout_book(&book, &styles, plain_registry(), &Assets::none());
    assert!(
        asking_for(&output.pages, b"sups").is_empty(),
        "the face has no superior figures to ask for",
    );
    let named: Vec<&str> = output
        .warnings
        .iter()
        .map(|warning| warning.message.as_str())
        .filter(|message| message.contains("superior figures"))
        .collect();
    assert_eq!(
        named,
        [
            "im fell english sc has no superior figures. The reference of a note stands on the \
          baseline."
        ],
        "{:?}",
        output.warnings,
    );
}

/// A note is a node of the book like any other: a session answers
/// which page it is set on and what box it takes there.
#[test]
fn a_session_answers_for_the_box_a_note_takes() {
    let book = read(&manuscript(4, 2, "note1 at the foot."));
    let mut session = fleuron::session::Session::new(registry());
    session.set_content(book.clone());
    session.set_style(Stylesheets::parse(&[]));
    session.preview();
    let note = fleuron::content::notes_in_blocks(&book.sections[0].blocks)
        .into_iter()
        .map(fleuron::content::inline_id)
        .next()
        .expect("the manuscript holds a note");
    let inside = book.subtree(note).expect("the note holds its own nodes");
    let boxes: Vec<_> = inside
        .map(fleuron::content::NodeId::new)
        .filter_map(|node| session.inspect(node))
        .flat_map(|inspection| inspection.boxes)
        .collect();
    assert!(
        !boxes.is_empty(),
        "the blocks of the note took no box on the page",
    );
    assert!(
        boxes.iter().all(|area| area.page == 0),
        "the note was set on another page than its reference",
    );
}

/// A manuscript with a note in each of its first few paragraphs.
fn manuscript_strategy() -> impl Strategy<Value = String> {
    (
        2usize..40,
        proptest::collection::vec(proptest::collection::vec("[a-z]{1,12}", 1..60), 1..5),
    )
        .prop_map(|(count, mut notes)| {
            notes.truncate(count);
            let mut out = String::new();
            for index in 1..=count {
                out.push_str(&format!("Line{index} of the manuscript runs along"));
                if index <= notes.len() {
                    out.push_str(&format!("[^{index}]"));
                }
                out.push_str(".\n\n");
            }
            for (index, note) in notes.iter().enumerate() {
                out.push_str(&format!("[^{}]: {}\n\n", index + 1, note.join(" ")));
            }
            out
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// The baselines of a page with a footnote area on it rise from
    /// the first line of the page to the last, the notes being the
    /// last of them.
    #[test]
    fn baselines_increase_down_a_page_with_notes(markdown in manuscript_strategy()) {
        let pages = lay_out(&markdown, "").pages;
        for page in &pages {
            let baselines: Vec<f32> = page
                .items
                .iter()
                .filter_map(|item| match item {
                    // The folio is the one run under the content box.
                    DrawItem::Text { y, size, .. } if *size > 8.5 => Some(*y),
                    _ => None,
                })
                .collect();
            let mut sorted = baselines.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).expect("no baseline is a nan"));
            prop_assert_eq!(&baselines, &sorted, "page {}", page.number);
        }
    }

    /// A book with notes lays out the same way twice, the numbering
    /// by page included.
    #[test]
    fn a_book_with_notes_lays_out_the_same_way_twice(markdown in manuscript_strategy()) {
        let css = "notes { counter-reset: note }";
        let first = lay_out(&markdown, css);
        let second = lay_out(&markdown, css);
        prop_assert_eq!(first.pages.len(), second.pages.len());
        for (a, b) in first.pages.iter().zip(&second.pages) {
            prop_assert_eq!(format!("{:?}", a.items), format!("{:?}", b.items));
        }
    }
}
