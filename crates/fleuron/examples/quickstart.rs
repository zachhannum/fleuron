//! The library quickstart, as a program: manuscript in, PDF out.
//!
//! `docs/library/quickstart.md` quotes this file, and a test holds the
//! two together.

use std::path::{Path, PathBuf};

use fleuron::fonts::bundled_registry;
use fleuron::images::{Assets, ImageLoader};
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::Options;

/// Resolves `@font-face` and image urls against one directory. The
/// engine reads no paths of its own, so the host supplies this half.
struct Files(PathBuf);

impl Files {
    fn read(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.read(url)
    }
}

impl ImageLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.read(url)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The frontend reads one source into sections. Assembly composes
    // sources into a book and numbers the tree in document order
    let source = "gulliver-excerpt.md";
    let markdown = std::fs::read_to_string(Path::new("fixtures").join(source))?;
    let (sections, complaints) =
        fleuron_markdown::to_sections(&markdown, source, &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections);

    // The built-in sheet is always first. Author sheets cascade over it.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let files = Files(PathBuf::from("fixtures"));
    let mut registry = bundled_registry()?;
    let mut sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    sheets.load_fonts(&mut registry, &files);
    let styles = sheets.compile(&book, &registry);

    // Every image the book content references.
    let assets = Assets::probe(&book, &files);

    let output = fleuron::layout::layout_book(&book, &styles, &registry, &assets);
    for warning in complaints.iter().chain(&output.warnings) {
        match &warning.origin {
            Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
            None => eprintln!("warning: {}", warning.message),
        }
    }

    // The PDF is painted from the structured output.
    let bytes = fleuron::pdf::write(&output, &registry, &assets, &book.metadata)?;
    std::fs::write(Path::new("book.pdf"), bytes)?;
    println!("{} pages", output.pages.len());
    Ok(())
}
