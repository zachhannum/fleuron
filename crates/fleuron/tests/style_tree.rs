//! Snapshot of the fixture book's compiled style tree: the distinct
//! computed styles, which element got which, and every page master
//! the built-in sheet resolves.

use fleuron::fonts::bundled_registry;
use fleuron_markdown::Options;

const MANUSCRIPT: &str = include_str!("../../../fixtures/gulliver-excerpt.md");

#[test]
fn fixture_book_style_tree_snapshot() {
    let registry = bundled_registry().expect("bundled font parses");
    let (sections, warnings) =
        fleuron_markdown::to_sections(MANUSCRIPT, "gulliver-excerpt.md", &Options::default());
    assert!(warnings.is_empty(), "{warnings:?}");
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(MANUSCRIPT), sections);
    let tree = fleuron::style::defaults(&book, &registry);
    assert!(tree.warnings().is_empty(), "{:?}", tree.warnings());
    insta::assert_json_snapshot!(tree);
}
