//! The `fleuron` binary: a manuscript in, a PDF out.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};

use fleuron::Warning;
use fleuron::content::{Book, HeadingLevel, Metadata};
use fleuron::images::{Assets, ImageLoader};
use fleuron::style::{FontLoader, Source, Stylesheets};
use fleuron_markdown::{Dialect, Options, Sections};

const USAGE: &str = "usage: fleuron <input.md…> -o <output.pdf> [-c <style.css>]

  -o, --output <path>  where to write the PDF
  -c, --css <path>     author stylesheet, cascading over the defaults;
                       repeatable, applied in the order given
  -s, --split <n|none> where a markdown file's sections begin: at a
                       heading of level n or shallower, or nowhere at
                       all, one section per file (default 1)
  -d, --dialect <name> commonmark, gfm or obsidian (default commonmark)
  -m, --metadata <path> the book's title and author, for a book that is
                       several files and so has no one file to read them
                       from
  -V, --version        print the version and exit
  -h, --help           print this message and exit

Markdown files compose in the order given. One markdown file is a whole
book, so its frontmatter is the book's; several are chapters, and each
file's frontmatter is its own. A single .json file is read as a content
tree instead, which is the same document one stage later.";

fn main() -> ExitCode {
    dispatch(std::env::args().skip(1)).code()
}

/// What the run tells the shell. Warnings are not failures: a book
/// that laid out with complaints still exits clean, having said so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    /// The command line named no job to do.
    Usage,
    /// The job was named and failed.
    Failure,
}

impl Status {
    fn code(self) -> ExitCode {
        match self {
            Status::Ok => ExitCode::SUCCESS,
            Status::Failure => ExitCode::FAILURE,
            Status::Usage => ExitCode::from(2),
        }
    }
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Render {
        inputs: Vec<PathBuf>,
        output: PathBuf,
        css: Vec<PathBuf>,
        /// Where the book's own metadata comes from, when no single
        /// input file speaks for the whole book.
        metadata: Option<PathBuf>,
        /// How the markdown frontend reads each file.
        reading: Options,
    },
    Version,
    Help,
}

fn dispatch(args: impl IntoIterator<Item = String>) -> Status {
    match parse(args) {
        Ok(Command::Version) => {
            println!("fleuron {}", env!("CARGO_PKG_VERSION"));
            Status::Ok
        }
        Ok(Command::Help) => {
            println!("{USAGE}");
            Status::Ok
        }
        Ok(Command::Render {
            inputs,
            output,
            css,
            metadata,
            reading,
        }) => match render(&inputs, &output, &css, metadata.as_deref(), &reading) {
            Ok(summary) => summary.report(&inputs, &output),
            Err(e) => {
                eprintln!("fleuron: {e:#}");
                Status::Failure
            }
        },
        Err(e) => {
            eprintln!("fleuron: {e}\n{USAGE}");
            Status::Usage
        }
    }
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Command> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut output: Option<PathBuf> = None;
    let mut css: Vec<PathBuf> = Vec::new();
    let mut metadata: Option<PathBuf> = None;
    let mut reading = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| anyhow!("{arg} needs a value"));
        match arg.as_str() {
            "-V" | "--version" => return Ok(Command::Version),
            "-h" | "--help" => return Ok(Command::Help),
            "-o" | "--output" => output = Some(PathBuf::from(value()?)),
            "-c" | "--css" => css.push(PathBuf::from(value()?)),
            "-m" | "--metadata" => metadata = Some(PathBuf::from(value()?)),
            "-s" | "--split" => reading.sections = split(&value()?)?,
            "-d" | "--dialect" => reading.dialect = dialect(&value()?)?,
            flag if flag.starts_with('-') && flag.len() > 1 => bail!("unknown option {flag}"),
            positional => inputs.push(PathBuf::from(positional)),
        }
    }
    if inputs.is_empty() {
        bail!("no input file");
    }
    // A content tree is a whole book already; there is no rule for
    // merging two of them that would not have to arbitrate metadata.
    if inputs.iter().any(|input| kind(input) == Some(Input::Tree)) && inputs.len() > 1 {
        bail!("one content tree at a time");
    }
    Ok(Command::Render {
        inputs,
        output: output.ok_or_else(|| anyhow!("no output path"))?,
        css,
        metadata,
        reading,
    })
}

/// Where a markdown file's sections begin, as the command line says
/// it.
fn split(value: &str) -> Result<Sections> {
    if value.eq_ignore_ascii_case("none") {
        return Ok(Sections::Whole);
    }
    value
        .parse::<u8>()
        .ok()
        .and_then(|level| HeadingLevel::try_from(level).ok())
        .map(Sections::AtHeading)
        .ok_or_else(|| anyhow!("--split takes a heading level 1-6, or none"))
}

fn dialect(value: &str) -> Result<Dialect> {
    match value.to_ascii_lowercase().as_str() {
        "commonmark" => Ok(Dialect::common_mark()),
        "gfm" => Ok(Dialect::gfm()),
        "obsidian" => Ok(Dialect::obsidian()),
        other => bail!("unknown dialect {other}"),
    }
}

/// What an input file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    Markdown,
    Tree,
}

/// What the extension says the file is. The engine reads two things,
/// and a name it does not recognise is a question rather than a
/// guess.
fn kind(path: &Path) -> Option<Input> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "md" | "markdown" => Some(Input::Markdown),
        "json" => Some(Input::Tree),
        _ => None,
    }
}

/// What one rendered book has to say for itself.
struct Summary {
    pages: usize,
    warnings: Vec<Warning>,
}

impl Summary {
    fn report(&self, inputs: &[PathBuf], output: &Path) -> Status {
        let named: Vec<String> = inputs.iter().map(|i| i.display().to_string()).collect();
        eprintln!(
            "fleuron: {} → {}: {} pages",
            named.join(", "),
            output.display(),
            self.pages,
        );
        for warning in &self.warnings {
            match &warning.origin {
                Some(origin) => eprintln!("fleuron: warning: {origin}: {}", warning.message),
                None => eprintln!("fleuron: warning: {}", warning.message),
            }
        }
        if !self.warnings.is_empty() {
            eprintln!(
                "fleuron: {} warning{}; the PDF was written anyway",
                self.warnings.len(),
                if self.warnings.len() == 1 { "" } else { "s" },
            );
        }
        Status::Ok
    }
}

fn render(
    inputs: &[PathBuf],
    output: &Path,
    css: &[PathBuf],
    metadata: Option<&Path>,
    reading: &Options,
) -> Result<Summary> {
    let mut registry = fleuron::fonts::bundled_registry().context("bundled font failed to load")?;
    let (book, mut warnings) = read_book(inputs, metadata, reading)?;

    let sheets = read_sheets(css)?;
    let sources: Vec<Source> = sheets
        .iter()
        .map(|(name, text)| Source::author(name, text))
        .collect();
    let mut stylesheets = Stylesheets::parse(&sources);
    // The engine opens nothing: `@font-face` and image urls are
    // resolved here, against the manuscript's directory and the
    // directory of the sheet that asked for them.
    let files = Files::rooted(inputs, css);
    stylesheets.load_fonts(&mut registry, &files);
    let styles = stylesheets.compile(&book, &registry);

    let assets = Assets::probe(&book, &files);
    let laid_out = fleuron::layout::layout_book_with_assets(&book, &styles, &registry, &assets);
    let bytes = fleuron::pdf::write(&laid_out, &registry, &book.metadata)
        .with_context(|| format!("{}", inputs[0].display()))?;
    std::fs::write(output, bytes).with_context(|| format!("{}", output.display()))?;
    warnings.extend(laid_out.warnings);
    Ok(Summary {
        pages: laid_out.pages.len(),
        warnings,
    })
}

/// Every author stylesheet, as `(name for diagnostics, text)`.
fn read_sheets(paths: &[PathBuf]) -> Result<Vec<(String, String)>> {
    paths
        .iter()
        .map(|path| {
            let text =
                std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
            Ok((path.display().to_string(), text))
        })
        .collect()
}

/// The host side of every url the engine does not open itself —
/// `@font-face` sources and images. Urls are paths, resolved against
/// the manuscript's own directory and the directories the stylesheets
/// came from.
struct Files {
    roots: Vec<PathBuf>,
}

impl Files {
    fn rooted(inputs: &[PathBuf], sheets: &[PathBuf]) -> Files {
        let mut roots: Vec<PathBuf> = inputs
            .iter()
            .chain(sheets)
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect();
        roots.push(PathBuf::from("."));
        roots.dedup();
        Files { roots }
    }

    fn read(&self, url: &str) -> Option<Vec<u8>> {
        self.roots
            .iter()
            .find_map(|root| std::fs::read(root.join(url)).ok())
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

/// Reads the inputs into one book, with whatever the reading had to
/// complain about.
///
/// Book metadata is the file `--metadata` names. Without one, a lone
/// markdown input is the whole book and its frontmatter is the book's.
/// Several inputs are chapters: each file's frontmatter belongs to the
/// section it became, and the book is left unnamed rather than named
/// after whichever chapter happened to come first.
fn read_book(
    inputs: &[PathBuf],
    metadata: Option<&Path>,
    reading: &Options,
) -> Result<(Book, Vec<Warning>)> {
    if let [tree] = inputs
        && kind(tree) == Some(Input::Tree)
    {
        return Ok((read_tree(tree)?, Vec::new()));
    }

    let mut sources = Vec::with_capacity(inputs.len());
    for input in inputs {
        if kind(input) != Some(Input::Markdown) {
            bail!("{}: not markdown or a content tree", input.display());
        }
        let text =
            std::fs::read_to_string(input).with_context(|| format!("{}", input.display()))?;
        sources.push((input.display().to_string(), text));
    }

    let metadata = match metadata {
        Some(path) => {
            let text =
                std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
            fleuron_markdown::metadata(&text)
        }
        None => match sources.as_slice() {
            [(_, text)] => fleuron_markdown::frontmatter(text),
            _ => Metadata::default(),
        },
    };

    let mut sections = Vec::new();
    let mut warnings = Vec::new();
    for (source, text) in &sources {
        let (read, complaints) = fleuron_markdown::to_sections(text, source, reading);
        sections.extend(read);
        warnings.extend(complaints);
    }
    Ok((fleuron_markdown::assemble(metadata, sections), warnings))
}

fn read_tree(input: &Path) -> Result<Book> {
    let text = std::fs::read_to_string(input).with_context(|| format!("{}", input.display()))?;
    parse_tree(&text, input)
}

fn parse_tree(text: &str, input: &Path) -> Result<Book> {
    let mut book: Book = serde_json::from_str(text)
        .with_context(|| format!("{}: not a content tree", input.display()))?;
    book.assign_node_ids();
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn render_of(words: &[&str]) -> Command {
        parse(args(words)).unwrap()
    }

    #[test]
    fn args_are_input_paths_and_an_output_pdf() {
        assert_eq!(
            render_of(&["manuscript.md", "-o", "out.pdf"]),
            Command::Render {
                inputs: vec![PathBuf::from("manuscript.md")],
                output: PathBuf::from("out.pdf"),
                css: Vec::new(),
                metadata: None,
                reading: Options::default(),
            },
        );
        assert_eq!(
            render_of(&["--output", "out.pdf", "book.json"]),
            Command::Render {
                inputs: vec![PathBuf::from("book.json")],
                output: PathBuf::from("out.pdf"),
                css: Vec::new(),
                metadata: None,
                reading: Options::default(),
            },
        );
        // Stylesheets cascade in the order the command line gives them.
        assert_eq!(
            render_of(&["a.md", "-o", "out.pdf", "-c", "a.css", "--css", "b.css"]),
            Command::Render {
                inputs: vec![PathBuf::from("a.md")],
                output: PathBuf::from("out.pdf"),
                css: vec![PathBuf::from("a.css"), PathBuf::from("b.css")],
                metadata: None,
                reading: Options::default(),
            },
        );
        for incomplete in [
            vec!["-o", "out.pdf"],
            vec!["book.md"],
            vec!["book.md", "-o"],
        ] {
            assert!(parse(args(&incomplete)).is_err(), "{incomplete:?}");
        }
    }

    /// Markdown composes; a content tree is a whole book already.
    #[test]
    fn several_markdown_files_compose_and_a_tree_stands_alone() {
        let Command::Render { inputs, .. } = render_of(&["two.md", "one.md", "-o", "out.pdf"])
        else {
            panic!("expected a render");
        };
        assert_eq!(inputs, [PathBuf::from("two.md"), PathBuf::from("one.md")]);
        assert!(parse(args(&["a.json", "b.json", "-o", "out.pdf"])).is_err());
        assert!(parse(args(&["a.md", "b.json", "-o", "out.pdf"])).is_err());
    }

    #[test]
    fn the_reading_is_named_on_the_command_line() {
        let Command::Render { reading, .. } = render_of(&[
            "book.md",
            "-o",
            "out.pdf",
            "--split",
            "2",
            "--dialect",
            "obsidian",
        ]) else {
            panic!("expected a render");
        };
        assert_eq!(reading.sections, Sections::AtHeading(HeadingLevel::H2));
        assert_eq!(reading.dialect, Dialect::obsidian());

        let Command::Render { reading, .. } =
            render_of(&["book.md", "-o", "out.pdf", "-s", "none"])
        else {
            panic!("expected a render");
        };
        assert_eq!(reading.sections, Sections::Whole);

        for bad in [
            vec!["book.md", "-o", "out.pdf", "-s", "7"],
            vec!["book.md", "-o", "out.pdf", "-s", "chapter"],
            vec!["book.md", "-o", "out.pdf", "-d", "asciidoc"],
        ] {
            assert!(parse(args(&bad)).is_err(), "{bad:?}");
        }
    }

    /// One file is a book and speaks for itself; several are chapters
    /// and do not.
    #[test]
    fn book_metadata_comes_from_one_file_or_from_the_flag() {
        let dir = std::env::temp_dir().join("fleuron-cli-metadata");
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let chapter = |name: &str, title: &str| {
            let path = dir.join(name);
            std::fs::write(
                &path,
                format!("---\ntitle: {title}\n---\n\n{title} opens here.\n"),
            )
            .expect("the chapter is writable");
            path
        };
        let one = chapter("meta-ch01.md", "The Ambassador");
        let two = chapter("meta-ch02.md", "A Cold Reception");
        let whole = Options {
            sections: Sections::Whole,
            ..Options::default()
        };

        // One file: its frontmatter is the book's.
        let (book, _) = read_book(std::slice::from_ref(&one), None, &whole).unwrap();
        assert_eq!(book.metadata.title.as_deref(), Some("The Ambassador"));

        // Several: the book is unnamed, and each chapter keeps its own
        // title rather than lending it to the work.
        let (book, _) = read_book(&[one.clone(), two.clone()], None, &whole).unwrap();
        assert_eq!(book.metadata.title, None);
        let chapters: Vec<Option<&str>> = book
            .sections
            .iter()
            .map(|section| section.title.as_deref())
            .collect();
        assert_eq!(chapters, [Some("The Ambassador"), Some("A Cold Reception")]);

        // Named explicitly, the work has a title and the chapters keep
        // theirs.
        let book_file = dir.join("meta-book.yaml");
        std::fs::write(&book_file, "title: The Levant Papers\nauthor: E. Marsh\n")
            .expect("the book file is writable");
        let (book, _) = read_book(&[one, two], Some(&book_file), &whole).unwrap();
        assert_eq!(book.metadata.title.as_deref(), Some("The Levant Papers"));
        assert_eq!(book.metadata.author.as_deref(), Some("E. Marsh"));
        assert_eq!(book.sections[0].title.as_deref(), Some("The Ambassador"));
    }

    #[test]
    fn version_and_help_are_commands_of_their_own() {
        for flag in ["-V", "--version"] {
            assert_eq!(parse(args(&[flag])).unwrap(), Command::Version);
        }
        for flag in ["-h", "--help"] {
            assert_eq!(parse(args(&[flag])).unwrap(), Command::Help);
        }
        // Asking for them is not a usage error, even with no job named.
        assert_eq!(dispatch(args(&["--version"])), Status::Ok);
        assert_eq!(dispatch(args(&["--help"])), Status::Ok);
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        let missing = [PathBuf::from("no/such/book.md")];
        let e = read_book(&missing, None, &Options::default()).unwrap_err();
        assert!(format!("{e:#}").contains("no/such/book.md"), "{e:#}");
    }

    #[test]
    fn an_unreadable_input_is_an_error_not_a_panic() {
        let e = parse_tree("{ not a content tree", Path::new("book.json")).unwrap_err();
        assert!(format!("{e:#}").contains("book.json"), "{e:#}");
        assert!(parse_tree("[]", Path::new("book.json")).is_err());
        let e = read_book(&[PathBuf::from("notes.txt")], None, &Options::default()).unwrap_err();
        assert!(format!("{e:#}").contains("notes.txt"), "{e:#}");
    }

    #[test]
    fn exit_codes_separate_clean_warned_and_failed_runs() {
        let clean = Summary {
            pages: 1,
            warnings: Vec::new(),
        };
        let warned = Summary {
            pages: 1,
            warnings: vec![Warning {
                message: "unsupported property".into(),
                origin: Some("node 12".into()),
            }],
        };
        let inputs = [PathBuf::from("book.md")];
        let out = Path::new("out.pdf");
        assert_eq!(clean.report(&inputs, out), Status::Ok);
        assert_eq!(warned.report(&inputs, out), Status::Ok);
        assert_eq!(dispatch(args(&[])), Status::Usage);
        assert_eq!(
            dispatch(args(&["no/such/book.md", "-o", "out.pdf"])),
            Status::Failure,
        );
        assert_eq!(Status::Ok.code(), ExitCode::SUCCESS);
        assert_eq!(Status::Failure.code(), ExitCode::FAILURE);
        assert_eq!(Status::Usage.code(), ExitCode::from(2));
    }
}
