//! Snapshot of the fixture book's compiled style tree: the distinct
//! computed styles, which element got which, and every page master
//! the built-in sheet resolves.

use fleuron::content::Book;
use fleuron::fonts::bundled_registry;

#[test]
fn fixture_book_style_tree_snapshot() {
    let registry = bundled_registry().expect("bundled font parses");
    let mut book: Book = serde_json::from_str(include_str!("../../../fixtures/book.json"))
        .expect("the fixture book parses");
    book.assign_node_ids();
    let tree = fleuron::style::defaults(&book, &registry);
    assert!(tree.warnings().is_empty(), "{:?}", tree.warnings());
    insta::assert_json_snapshot!(tree);
}
