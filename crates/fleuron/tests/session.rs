//! Property tests for the retained session. Whatever sequence of
//! edits got it there, a session's output is the output a one-shot
//! run over the same inputs would have produced.

use fleuron::content::{Block, Book, HeadingLevel, Inline, Metadata, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::layout_book;
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};
use proptest::prelude::*;

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

fn heading(value: &str) -> Block {
    Block::Heading {
        id: NodeId::UNASSIGNED,
        level: HeadingLevel::H1,
        inlines: vec![text(value)],
        position: None,
    }
}

fn paragraph(value: &str) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![text(value)],
        position: None,
    }
}

/// One section of `paragraphs` paragraphs, long enough to break over
/// lines and, in quantity, over pages.
fn section(source: &str, tag: &str, paragraphs: usize) -> Section {
    Section {
        id: NodeId::UNASSIGNED,
        source: Some(source.into()),
        title: None,
        blocks: std::iter::once(heading(tag))
            .chain((0..paragraphs).map(|index| {
                paragraph(&format!(
                    "{}{index}.",
                    format!("{tag} word and another {tag} word ").repeat(6)
                ))
            }))
            .collect(),
        position: None,
    }
}

/// Replacing one file re-breaks that file and nothing else.
///
/// The property tests prove a session's output is right. This proves
/// what it did not do to get there, which a clock cannot show: the
/// nine sections the edit did not touch keep the lines they already
/// had, and the book is fragmented once over the result.
#[test]
fn replacing_one_source_re_breaks_only_that_source() {
    let shape: Vec<(String, String, usize)> = (0..10)
        .map(|index| (format!("ch{index:02}.md"), format!("tag{index}"), 6))
        .collect();
    let borrowed: Vec<(&str, &str, usize)> = shape
        .iter()
        .map(|(name, tag, paragraphs)| (name.as_str(), tag.as_str(), *paragraphs))
        .collect();

    let mut session = Session::new(registry());
    session.set_content(book(&borrowed));
    session.set_style(Stylesheets::parse(&[]));
    session.preview();
    let first = session.stages();
    assert_eq!(first.lines, 10, "the first preview breaks every section");

    session.replace_source("ch05.md", vec![section("ch05.md", "edited", 6)]);
    session.preview();
    let edited = session.stages();

    assert_eq!(
        edited.lines - first.lines,
        1,
        "an edit to one file re-broke {} sections",
        edited.lines - first.lines,
    );
    assert_eq!(edited.flow - first.flow, 1, "the book flows once per edit");
}

fn book(shape: &[(&str, &str, usize)]) -> Book {
    Book {
        metadata: Metadata {
            title: Some("A Session".into()),
            ..Metadata::default()
        },
        sections: shape
            .iter()
            .map(|(source, tag, paragraphs)| section(source, tag, *paragraphs))
            .collect(),
    }
}

/// A page small enough that a handful of paragraphs spans several of
/// them. What the tiers do to page geometry is only visible on a book
/// whose pages break.
const PAGE: &str = "@page { size: 300pt 300pt }";

/// The sheets a source compiles to, over the small page.
fn parse(css: &str) -> Stylesheets {
    Stylesheets::parse(&[
        Source::author("page.css", PAGE),
        Source::author("session.css", css),
    ])
}

/// The one-shot run over the same inputs: the answer the session has
/// to match.
fn one_shot(book: &Book, css: &str) -> Vec<u8> {
    let mut book = book.clone();
    book.assign_node_ids();
    let sheets = parse(css);
    let styles = sheets.compile(&book, registry());
    let output = layout_book(&book, &styles, registry());
    serde_json::to_vec(&output).expect("the output serializes")
}

/// What a host does between previews. Each edit includes everything
/// the session needs to apply it, so the test can replay the same
/// edits against a book of its own.
#[derive(Debug, Clone)]
enum Edit {
    /// A whole book, as a shape.
    Content(usize),
    /// One file's sections, replaced.
    Source(usize),
    /// A stylesheet, by index into `SHEETS`.
    Style(usize),
    /// A preview taken mid-sequence, which is what leaves the caches
    /// something to go stale.
    Preview,
}

/// The books an edit can set: different section counts, orders and
/// lengths, over the same three file names.
const BOOKS: [&[(&str, &str, usize)]; 4] = [
    &[("one.md", "alpha", 3), ("two.md", "beta", 4)],
    &[
        ("one.md", "alpha", 5),
        ("two.md", "beta", 2),
        ("three.md", "gamma", 6),
    ],
    &[("three.md", "gamma", 6), ("one.md", "alpha", 5)],
    &[("two.md", "beta", 9)],
];

/// The replacements a source edit can make.
const SOURCES: [(&str, &str, usize); 4] = [
    ("one.md", "delta", 2),
    ("two.md", "epsilon", 7),
    ("three.md", "zeta", 1),
    ("four.md", "eta", 3),
];

/// Sheets spanning every tier the classifier has: one the engine
/// models nothing of, one that only moves furniture, one that moves
/// the page box, two that move the measure and the face, and one
/// that puts two measures on the book at once.
const SHEETS: [&str; 6] = [
    "p { color: rebeccapurple }",
    "@page { @bottom-center { content: \"leaf\" } }",
    "@page { margin-bottom: 96pt }",
    "book { font-size: 13pt; line-height: 1.6 }",
    "@page { margin-left: 90pt }",
    "@page :left { margin-left: 24pt }",
];

fn edit_strategy() -> impl Strategy<Value = Edit> {
    prop_oneof![
        (0..BOOKS.len()).prop_map(Edit::Content),
        (0..SOURCES.len()).prop_map(Edit::Source),
        (0..SHEETS.len()).prop_map(Edit::Style),
        Just(Edit::Preview),
    ]
}

/// The book an edit sequence leaves, built without the session's
/// help: the same rule, written a second time.
fn replay(edits: &[Edit]) -> (Book, &'static str) {
    let mut book = Book::default();
    // A sheet of nothing is where the session starts: the small page,
    // and no author rule over it.
    let mut css = "";
    for edit in edits {
        match edit {
            Edit::Content(index) => book = self::book(BOOKS[*index]),
            Edit::Source(index) => {
                let (name, tag, paragraphs) = SOURCES[*index];
                let fresh = section(name, tag, paragraphs);
                let mut placed = false;
                let mut rebuilt = Vec::new();
                for section in std::mem::take(&mut book.sections) {
                    if section.source.as_deref() == Some(name) {
                        if !placed {
                            rebuilt.push(fresh.clone());
                            placed = true;
                        }
                    } else {
                        rebuilt.push(section);
                    }
                }
                if !placed {
                    rebuilt.push(fresh);
                }
                book.sections = rebuilt;
            }
            Edit::Style(index) => css = SHEETS[*index],
            Edit::Preview => {}
        }
    }
    (book, css)
}

/// Every sheet, previewed, then every sheet again. Whichever tier
/// the classifier picked for that step, the bytes are the ones a
/// one-shot run over the second sheet would have produced.
///
/// The property test wanders, and this covers the matrix.
#[test]
fn every_style_step_lands_where_a_one_shot_run_would() {
    let shape = BOOKS[1];
    for first in SHEETS {
        for next in SHEETS {
            let mut session = Session::new(registry());
            session.set_content(book(shape));
            session.set_style(parse(first));
            session.preview();
            session.set_style(parse(next));
            let retained = serde_json::to_vec(session.preview()).expect("the output serializes");
            assert_eq!(
                retained,
                one_shot(&book(shape), next),
                "{first} then {next}"
            );
        }
    }
}

/// Every sheet followed by a file replaced. The sections that kept
/// their lines have to be the sections a fresh run would have broken
/// the same way.
#[test]
fn every_source_edit_lands_where_a_one_shot_run_would() {
    let shape = BOOKS[1];
    for css in SHEETS {
        for (name, tag, paragraphs) in SOURCES {
            let mut session = Session::new(registry());
            session.set_content(book(shape));
            session.set_style(parse(css));
            session.preview();
            session.replace_source(name, vec![section(name, tag, paragraphs)]);
            let retained = serde_json::to_vec(session.preview()).expect("the output serializes");

            let (expected, _) = replay(&[
                Edit::Content(1),
                Edit::Source(
                    SOURCES
                        .iter()
                        .position(|source| source.0 == name)
                        .expect("the replacement is one of the four"),
                ),
            ]);
            assert_eq!(retained, one_shot(&expected, css), "{css} then {name}");
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// A session's output is byte-identical to a one-shot run over
    /// the inputs it ended up with, however it got there: content set
    /// and reset, one file replaced, sheets swapped, previews taken
    /// along the way.
    #[test]
    fn a_session_ends_where_a_one_shot_run_would_have(
        edits in proptest::collection::vec(edit_strategy(), 1..8),
    ) {
        let mut session = Session::new(registry());
        session.set_style(parse(""));
        for edit in &edits {
            match edit {
                Edit::Content(index) => session.set_content(book(BOOKS[*index])),
                Edit::Source(index) => {
                    let (name, tag, paragraphs) = SOURCES[*index];
                    session.replace_source(name, vec![section(name, tag, paragraphs)]);
                }
                Edit::Style(index) => {
                    session.set_style(parse(SHEETS[*index]));
                }
                Edit::Preview => {
                    session.preview();
                }
            }
        }

        let (expected_book, css) = replay(&edits);
        prop_assert_eq!(
            session.book().sections.len(),
            expected_book.sections.len(),
            "the session and the replay disagree about the book"
        );
        let retained = serde_json::to_vec(session.preview()).expect("the output serializes");
        prop_assert_eq!(retained, one_shot(&expected_book, css));
    }

    /// The session is deterministic in its own right: the same edits
    /// twice over two sessions leave the same bytes.
    #[test]
    fn the_same_edits_leave_the_same_pages(
        edits in proptest::collection::vec(edit_strategy(), 1..6),
    ) {
        let run = |edits: &[Edit]| {
            let mut session = Session::new(registry());
            session.set_style(parse(""));
            for edit in edits {
                match edit {
                    Edit::Content(index) => session.set_content(book(BOOKS[*index])),
                    Edit::Source(index) => {
                        let (name, tag, paragraphs) = SOURCES[*index];
                        session.replace_source(name, vec![section(name, tag, paragraphs)]);
                    }
                    Edit::Style(index) => {
                        session.set_style(parse(SHEETS[*index]));
                    }
                    Edit::Preview => {
                        session.preview();
                    }
                }
            }
            serde_json::to_vec(session.preview()).expect("the output serializes")
        };
        prop_assert_eq!(run(&edits), run(&edits));
    }
}
