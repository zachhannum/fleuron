//! A character the face has no glyph for: the page has a box where
//! the author wrote a letter, and the run says so.

use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::session::Session;
use fleuron::style::{Source, Stylesheets};
use fleuron::{LayoutOutput, wire};
use fleuron_markdown::Options;

const GULLIVER: &str = include_str!("../../../fixtures/gulliver-excerpt.md");

/// Two files. Both have the star twice, and the second has a
/// Japanese word as well.
const SOURCES: [(&str, &str); 2] = [
    (
        "one.md",
        "# One\n\nA plain line.\nFive ★ stars, and one ★ more.\n",
    ),
    (
        "two.md",
        "# Two\n\nThe word *is 日本* here.\n\n## Under\n\nRated `a ★` again, and ★.\n",
    ),
];

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn book(sources: &[(&str, &str)]) -> Book {
    let mut sections = Vec::new();
    for (name, markdown) in sources {
        let (read, warnings) = fleuron_markdown::to_sections(markdown, name, &Options::default());
        assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
        sections.extend(read);
    }
    fleuron_markdown::assemble(Default::default(), sections)
}

fn sheets() -> Stylesheets {
    Stylesheets::parse(&[Source::author("book.css", "")])
}

fn one_shot(book: &Book) -> LayoutOutput {
    let styles = sheets().compile(book, registry());
    layout_book(book, &styles, registry(), &Assets::none())
}

/// Every warning about a glyph, as the character it names and where
/// it points.
fn uncovered(output: &LayoutOutput) -> Vec<(char, &str)> {
    output
        .warnings
        .iter()
        .filter(|warning| warning.message.contains("has no glyph for"))
        .map(|warning| {
            let character = warning
                .message
                .split('`')
                .nth(1)
                .and_then(|named| named.chars().next())
                .expect("the warning names a character");
            (
                character,
                warning
                    .origin
                    .as_deref()
                    .expect("the warning has an origin"),
            )
        })
        .collect()
}

/// Acceptance: a character no registered face covers warns once for
/// each character and source, with the character and the line and
/// column it was written at.
#[test]
fn an_uncovered_character_warns_once_for_each_character_and_source() {
    let book = book(&SOURCES);
    let expected = [
        ('★', "one.md:4:6"),
        ('日', "two.md:3:14"),
        ('本', "two.md:3:17"),
        ('★', "two.md:7:10"),
    ];
    let output = one_shot(&book);
    assert_eq!(uncovered(&output), expected, "{:?}", output.warnings);
    let star = output
        .warnings
        .iter()
        .find(|warning| warning.message.contains('★'))
        .expect("the star warns");
    assert!(star.message.contains("U+2605"), "{}", star.message);
    assert!(star.message.contains("EB Garamond"), "{}", star.message);

    // A session keeps each section's lines, and answers the same.
    let mut session = Session::new(registry());
    session.set_style(sheets());
    session.set_content(book);
    assert_eq!(session.preview().warnings, output.warnings);
}

/// Acceptance: a book in which every character has a glyph reports
/// no such warning. The bundled face has Greek and Cyrillic, and a
/// hard break and a soft hyphen are not characters it has to draw.
#[test]
fn a_book_the_face_covers_reports_nothing() {
    let covered = one_shot(&book(&[("book.md", GULLIVER)]));
    assert_eq!(uncovered(&covered), [], "{:?}", covered.warnings);
    assert!(covered.pages.len() > 1);

    let scripts = "# Ἰλιάς\n\nВойна и мир, naïve café.  \nA co\u{ad}operative line.\n\n\
                   ```\nfn main() {\n\tprintln!(\"—\");\n}\n```\n";
    let scripts = one_shot(&book(&[("scripts.md", scripts)]));
    assert_eq!(scripts.warnings, [], "{:?}", scripts.warnings);
}

/// Acceptance: the warning crosses the wire with an origin, like any
/// other warning.
#[test]
fn the_warning_crosses_the_wire_with_its_origin() {
    let output = one_shot(&book(&SOURCES));
    let bytes = wire::encode(&output).expect("a display structure encodes");
    let read = wire::decode(&bytes).expect("a display structure decodes");
    assert_eq!(read.warnings, output.warnings);
    let star = read
        .warnings
        .iter()
        .find(|warning| warning.message.contains('★'))
        .expect("the star crossed");
    assert_eq!(star.origin.as_deref(), Some("one.md:4:6"));
}

/// A character in a code block is reported at the block, whose text
/// runs over lines of its own.
#[test]
fn a_character_in_a_code_block_is_reported_at_the_block() {
    let output = one_shot(&book(&[(
        "code.md",
        "# C\n\nProse.\n\n```\none\ntwo ★\n```\n",
    )]));
    assert_eq!(uncovered(&output), [('★', "code.md:5:1")]);
}
