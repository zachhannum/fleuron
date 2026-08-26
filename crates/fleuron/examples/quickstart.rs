//! The library quickstart, as a program: manuscript in, PDF out.
//!
//! `docs/library/quickstart.md` quotes this file, and a test holds the
//! two together.

use std::path::{Path, PathBuf};

use fleuron::fonts::bundled_registry;
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::Options;

/// Resolves `@font-face` urls against one directory. The engine reads
/// no paths of its own; this is the host half of that contract.
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Content enters as markdown. The frontend reads one source into
    // sections; assembly composes the sources into a book and numbers
    // it in document order, which is what diagnostics point at.
    let source = "gulliver-excerpt.md";
    let markdown = std::fs::read_to_string(Path::new("fixtures").join(source))?;
    let (sections, complaints) =
        fleuron_markdown::to_sections(&markdown, source, &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections);

    // Styling enters as CSS. The built-in sheet is always first;
    // author sheets cascade over it in the order given.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let mut registry = bundled_registry()?;
    let mut sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    sheets.load_fonts(&mut registry, &Files(PathBuf::from("fixtures")));
    let styles = sheets.compile(&book, &registry);

    // One call from styled tree to pages of draw items.
    let output = fleuron::layout::layout_book(&book, &styles, &registry);
    for warning in complaints.iter().chain(&output.warnings) {
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
