//! The names a sheet reaches an element by, as a host reads them.
//!
//! The tree is how a host reads what a frontend made of a
//! manuscript, and what a host with a structured source of its own
//! builds. The classes and the id are checked both ways: the shape
//! they serialize to is under snapshot, and what the engine wrote
//! reads back as the tree it wrote it from.

use fleuron::content::Book;
use fleuron_markdown::{Options, assemble, to_sections};

/// Every way of naming a block in one manuscript: the line above a
/// quote, a heading's trailing run, the run after an image alone on
/// its line, and the line above a scene break.
const MANUSCRIPT: &str = "\
# Chapter One {#opening .grand}

{.epigraph}
> Man is the only animal that blushes.

![a map of Lilliput](plate.jpg){.plate #frontispiece}

{.ornament}
---

Plain prose carries nothing.
";

fn read() -> Book {
    let (sections, warnings) = to_sections(MANUSCRIPT, "chapter-01.md", &Options::default());
    assert!(warnings.is_empty(), "{warnings:?}");
    assemble(Default::default(), sections)
}

#[test]
fn a_named_tree_serializes_to_the_shape_it_is_checked_in_as() {
    let json = serde_json::to_string_pretty(&read()).expect("the tree serializes");
    insta::assert_snapshot!(json);
}

#[test]
fn what_the_engine_wrote_reads_back_named() {
    let book = read();
    let json = serde_json::to_string(&book).expect("the tree serializes");
    let again: Book = serde_json::from_str(&json).expect("the tree reads back");
    // Ids are the engine's, and a serialized tree has none.
    let mut again = again;
    again.assign_node_ids();
    assert_eq!(again, book);
}
