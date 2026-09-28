//! The classes and the ids a session reports for a markdown source.
//!
//! The answer comes from the tree the frontend read, so it agrees with
//! what a sheet can select: a brace run that stays prose names
//! nothing, and a dialect with attributes off names nothing at all.

use fleuron::content::{Metadata, Names};
use fleuron::fonts::bundled_registry;
use fleuron::session::Session;
use fleuron_markdown::{Dialect, Options, assemble, to_sections};

const SOURCE: &str = "# Chapter One {.opening #ch1}

{.epigraph}
> It is a truth universally acknowledged.

The first paragraph.
";

fn names(sources: &[(&str, &str)], options: &Options) -> Names {
    let registry = bundled_registry().unwrap();
    let mut session = Session::new(&registry);
    let sections = sources
        .iter()
        .flat_map(|(name, text)| to_sections(text, name, options).0)
        .collect();
    session.set_content(assemble(Metadata::default(), sections));
    session.names(Some("chapter.md"))
}

/// Acceptance: a source with `{.epigraph}` before a quote and
/// `# Chapter One {.opening #ch1}` reports the classes `epigraph` and
/// `opening` and the id `ch1`.
#[test]
fn a_source_reports_the_classes_and_the_id_it_writes() {
    let names = names(&[("chapter.md", SOURCE)], &Options::default());
    assert_eq!(names.classes, ["epigraph", "opening"]);
    assert_eq!(names.ids, ["ch1"]);
}

/// Acceptance: after an edit removes the attribute line, the class
/// `epigraph` is no longer reported.
#[test]
fn an_edit_that_removes_the_attribute_line_removes_the_class() {
    let registry = bundled_registry().unwrap();
    let options = Options::default();
    let mut session = Session::new(&registry);
    let (sections, _) = to_sections(SOURCE, "chapter.md", &options);
    session.set_content(assemble(Metadata::default(), sections));
    assert!(
        session
            .names(Some("chapter.md"))
            .classes
            .contains(&"epigraph".to_string())
    );

    let edited = SOURCE.replace("{.epigraph}\n", "");
    let (sections, _) = to_sections(&edited, "chapter.md", &options);
    session.replace_source("chapter.md", sections);
    let names = session.names(Some("chapter.md"));
    assert_eq!(names.classes, ["opening"]);
    assert_eq!(names.ids, ["ch1"]);
}

/// Acceptance: under CommonMark, where attributes are off, the same
/// source reports no class and no id.
#[test]
fn commonmark_reports_no_name() {
    let options = Options {
        dialect: Dialect::common_mark(),
        ..Options::default()
    };
    assert_eq!(names(&[("chapter.md", SOURCE)], &options), Names::default());
}

/// Acceptance: an attribute line that annotates nothing reports no
/// name. The source ending under it and a second line under it both
/// leave it prose, and so does a brace run that is not classes and an
/// id.
#[test]
fn an_attribute_line_that_names_nothing_reports_no_name() {
    let options = Options::default();
    let dangling = "The last paragraph.\n\n{.coda}\n";
    assert_eq!(
        names(&[("chapter.md", dangling)], &options),
        Names::default()
    );

    let doubled = "{.first}\n\n{.second}\n\nProse.\n";
    assert_eq!(
        names(&[("chapter.md", doubled)], &options).classes,
        ["second"]
    );

    let prose = "{.aside key=value}\n\nProse.\n";
    assert_eq!(names(&[("chapter.md", prose)], &options), Names::default());
}

/// Part: a source answers for its own blocks, and no source answers
/// for the book.
#[test]
fn a_source_answers_for_itself_and_none_for_the_book() {
    let registry = bundled_registry().unwrap();
    let options = Options::default();
    let mut session = Session::new(&registry);
    let sections = [
        ("chapter.md", SOURCE),
        ("coda.md", "# Coda {.closing #end}\n"),
    ]
    .iter()
    .flat_map(|(name, text)| to_sections(text, name, &options).0)
    .collect();
    session.set_content(assemble(Metadata::default(), sections));

    assert_eq!(session.names(Some("coda.md")).classes, ["closing"]);
    let book = session.names(None);
    assert_eq!(book.classes, ["closing", "epigraph", "opening"]);
    assert_eq!(book.ids, ["ch1", "end"]);
}
