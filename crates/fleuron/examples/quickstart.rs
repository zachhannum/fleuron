//! The library quickstart, as a program: fixture book in, PDF out.
//!
//! `docs/library/quickstart.md` quotes this file, and a test holds the
//! two together.

use std::path::{Path, PathBuf};

use fleuron::content::Book;
use fleuron::fonts::bundled_registry;
use fleuron::style::{FontLoader, Source, Stylesheets};

/// Resolves `@font-face` urls against one directory. The engine reads
/// no paths of its own; this is the host half of that contract.
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Content enters as a tree. `assign_node_ids` numbers it in
    // document order, which is what diagnostics point at.
    let json = std::fs::read_to_string("fixtures/book.json")?;
    let mut book: Book = serde_json::from_str(&json)?;
    book.assign_node_ids();

    // Styling enters as CSS. The built-in sheet is always first;
    // author sheets cascade over it in the order given.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let mut registry = bundled_registry()?;
    let mut sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    sheets.load_fonts(&mut registry, &Files(PathBuf::from("fixtures")));
    let styles = sheets.compile(&book, &registry);

    // One call from styled tree to pages of draw items.
    let output = fleuron::layout::layout_book(&book, &styles, &registry);
    for warning in &output.warnings {
        match &warning.origin {
            Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
            None => eprintln!("warning: {}", warning.message),
        }
    }

    // The PDF is painted from the display list, not laid out again.
    let bytes = fleuron::pdf::write(&output, &registry, &book.metadata)?;
    std::fs::write(Path::new("book.pdf"), bytes)?;
    println!("{} pages", output.pages.len());
    Ok(())
}
