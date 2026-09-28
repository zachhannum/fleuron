//! The program `docs/reference/epub.md` quotes, and a test keeps the
//! two the same. Run it from the root of the repository.

use std::path::{Path, PathBuf};

use fleuron::images::ImageLoader;
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::Options;

/// Resolves `@font-face` and image urls against one directory.
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
    let source = "gulliver-excerpt.md";
    let markdown = std::fs::read_to_string(Path::new("fixtures").join(source))?;
    let (sections, complaints) =
        fleuron_markdown::to_sections(&markdown, source, &Options::default());
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(&markdown), sections);

    // The same sheets as the PDF. Nothing is laid out, so there is no
    // font registry and no style tree.
    let css = std::fs::read_to_string("fixtures/styled.css")?;
    let sheets = Stylesheets::parse(&[Source::author("styled.css", &css)]);
    let files = Files(PathBuf::from("fixtures"));

    let epub = fleuron_epub::write(&book, &sheets, &files, &files);
    for warning in complaints.iter().chain(&epub.warnings) {
        match &warning.origin {
            Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
            None => eprintln!("warning: {}", warning.message),
        }
    }
    std::fs::write("book.epub", &epub.bytes)?;
    Ok(())
}
