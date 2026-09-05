//! Hyphenation in the language the book declares: German words at
//! German points, French at French, and a tag nothing answers to
//! left whole with a word about it.

use fleuron::LayoutOutput;
use fleuron::content::{Block, Book, Inline, Metadata, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::DrawItem;
use fleuron::style::{Source, Stylesheets};

/// A page 45pt across, hyphenated: a measure narrow enough that
/// which syllables a language has is what decides where a line ends.
const CSS: &str = r#"
@page { size: 45pt 400pt; margin: 12pt }

p { hyphens: auto }
"#;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// One paragraph of prose, in a book that declares `language` or
/// declares none.
fn book(language: Option<&str>, prose: &str) -> Book {
    let mut book = Book {
        metadata: Metadata {
            extra: language
                .map(|tag| ("language".to_string(), tag.to_string()))
                .into_iter()
                .collect(),
            ..Default::default()
        },
        sections: vec![Section {
            blocks: vec![Block::Paragraph {
                id: NodeId::UNASSIGNED,
                inlines: vec![Inline::Text {
                    id: NodeId::UNASSIGNED,
                    value: prose.to_string(),
                    position: None,
                }],
                position: None,
            }],
            ..Default::default()
        }],
    };
    book.assign_node_ids();
    book
}

fn lay_out(book: &Book) -> LayoutOutput {
    let styles =
        Stylesheets::parse(&[Source::author("hyphenation.css", CSS)]).compile(book, registry());
    layout_book(book, &styles, registry(), &Assets::none())
}

/// The painted lines, in reading order: the runs sharing a baseline
/// joined, so a line that ends hyphenated ends in the hyphen.
fn lines(output: &LayoutOutput) -> Vec<String> {
    let mut lines: Vec<(f32, String)> = Vec::new();
    for page in &output.pages {
        for item in &page.items {
            if let DrawItem::Text { y, text, .. } = item {
                match lines.last_mut() {
                    Some((at, line)) if at == y => line.push_str(text),
                    _ => lines.push((*y, text.clone())),
                }
            }
        }
    }
    lines.into_iter().map(|(_, line)| line).collect()
}

/// A German book breaks `Wassermann` where German breaks it. English
/// has a pattern for the seam between the two words it is made of and
/// none for anything inside them, which is the break the word gets
/// today.
#[test]
fn german_words_break_at_german_points() {
    let word = "Wassermann";
    assert_eq!(
        lines(&lay_out(&book(Some("de"), word))),
        ["Was-", "ser-", "mann"]
    );
    assert_eq!(
        lines(&lay_out(&book(Some("en"), word))),
        ["Wasser-", "mann"]
    );
}

/// A regional tag reads as its primary subtag, so `fr-CA` is French,
/// and French breaks `malheureusement` at syllables English has no
/// pattern for.
#[test]
fn a_regional_tag_hyphenates_as_its_language() {
    let word = "malheureusement";
    let french = ["mal-", "heu-", "reuse-", "ment"];
    assert_eq!(lines(&lay_out(&book(Some("fr-CA"), word))), french);
    assert_eq!(lines(&lay_out(&book(Some("fr"), word))), french);
    assert_eq!(
        lines(&lay_out(&book(Some("en"), word))),
        ["mal-", "heureuse-", "ment"]
    );
}

/// A tag with no patterns behind it warns, names itself, and leaves
/// every word whole rather than breaking one language's words at
/// another's syllables.
#[test]
fn an_unknown_language_warns_once_and_breaks_nothing() {
    let output = lay_out(&book(Some("xx"), "Wassermann malheureusement"));

    let named: Vec<&str> = output
        .warnings
        .iter()
        .map(|warning| warning.message.as_str())
        .filter(|message| message.contains("xx"))
        .collect();
    assert_eq!(named.len(), 1, "warnings: {:?}", output.warnings);
    assert!(
        named[0].contains("hyphenation"),
        "the warning says nothing about hyphenation: {}",
        named[0]
    );
    assert_eq!(lines(&output), ["Wassermann", "malheureusement"]);
}

/// A book that declares no language hyphenates as one that declares
/// English: the behaviour every book had before a language chose the
/// patterns.
#[test]
fn a_book_that_declares_no_language_breaks_as_english() {
    let prose = "extraordinarily inconsiderate lamplighter incomprehensible";
    assert_eq!(
        lines(&lay_out(&book(None, prose))),
        lines(&lay_out(&book(Some("en"), prose))),
    );
}

/// The corpus novel, hyphenated end to end, breaks the same whether
/// it declares English or declares nothing at all.
#[test]
fn the_corpus_novel_breaks_the_same_under_english() {
    use fleuron_fixtures::Corpus;

    let silent = Corpus::PrideAndPrejudice.book();
    let mut declared = silent.clone();
    declared
        .metadata
        .extra
        .insert("language".to_string(), "en".to_string());

    let styles = Stylesheets::parse(&[Source::author("hyphenation.css", "p { hyphens: auto }")])
        .compile(&silent, registry());
    let pages = |book: &Book| layout_book(book, &styles, registry(), &Assets::none()).pages;
    assert_eq!(pages(&declared), pages(&silent));
}

/// A word past the 45 bytes the hyphenator holds inline is
/// hyphenated all the same. Two dozen Cyrillic letters are 48 bytes,
/// which an ordinary Russian noun reaches.
#[test]
fn a_word_longer_than_the_inline_buffer_hyphenates() {
    let word = "человеконенавистничество";
    assert!(word.len() > 45, "the word is {} bytes", word.len());
    assert!(!lines(&lay_out(&book(Some("ru"), word))).is_empty());
}
