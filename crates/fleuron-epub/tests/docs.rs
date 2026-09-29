//! The EPUB page and the example it quotes are one program, and drift
//! between them is a test failure.

use std::path::Path;

#[test]
fn the_epub_page_quotes_the_example_it_runs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let example = std::fs::read_to_string(root.join("crates/fleuron-epub/examples/epub.rs"))
        .expect("the example");
    let page = std::fs::read_to_string(root.join("docs/library/epub.md")).expect("the doc page");

    // The example's own `//!` header stays out of the page.
    let code: String = example
        .lines()
        .skip_while(|line| line.starts_with("//!") || line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let quoted = page
        .split("```rust\n")
        .nth(1)
        .and_then(|rest| rest.split("\n```").next())
        .expect("a rust block on the page");

    assert_eq!(quoted.trim(), code.trim());
}
