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
  --title <text>       the book's title
  --author <text>      the book's author
  --meta <key=value>   any other metadata field; repeatable. `language`
                       is the one the PDF writer reads
  --dump-tree          write the content tree the frontend read to
                       stdout as JSON, and lay nothing out
  -V, --version        print the version and exit
  -h, --help           print this message and exit

Markdown files compose in the order given. One markdown file is a whole
book, so its frontmatter is the book's; several are chapters, and each
file's frontmatter is its own, which is what --title and --author are
for.";

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
#[derive(Debug, PartialEq)]
enum Command {
    Render {
        inputs: Vec<PathBuf>,
        output: PathBuf,
        css: Vec<PathBuf>,
        /// The book's own metadata, named on the command line. Fields
        /// left unset here fall back to frontmatter when one input
        /// speaks for the whole book.
        metadata: Metadata,
        /// How the markdown frontend reads each file.
        reading: Options,
    },
    /// Write the tree the frontend read, and lay nothing out.
    DumpTree {
        inputs: Vec<PathBuf>,
        metadata: Metadata,
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
        }) => match render(&inputs, &output, &css, metadata, &reading) {
            Ok(summary) => summary.report(&inputs, &output),
            Err(e) => {
                eprintln!("fleuron: {e:#}");
                Status::Failure
            }
        },
        Ok(Command::DumpTree {
            inputs,
            metadata,
            reading,
        }) => match dump_tree(&inputs, metadata, &reading) {
            Ok(warnings) => {
                report_warnings(&warnings);
                Status::Ok
            }
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
    let mut metadata = Metadata::default();
    let mut reading = Options::default();
    let mut dump = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| anyhow!("{arg} needs a value"));
        match arg.as_str() {
            "-V" | "--version" => return Ok(Command::Version),
            "-h" | "--help" => return Ok(Command::Help),
            "-o" | "--output" => output = Some(PathBuf::from(value()?)),
            "-c" | "--css" => css.push(PathBuf::from(value()?)),
            "--title" => metadata.title = Some(value()?),
            "--author" => metadata.author = Some(value()?),
            "--meta" => {
                let field = value()?;
                let (key, held) = field
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--meta takes key=value, got {field}"))?;
                metadata.extra.insert(key.to_string(), held.to_string());
            }
            "--dump-tree" => dump = true,
            "-s" | "--split" => reading.sections = split(&value()?)?,
            "-d" | "--dialect" => reading.dialect = dialect(&value()?)?,
            flag if flag.starts_with('-') && flag.len() > 1 => bail!("unknown option {flag}"),
            positional => inputs.push(PathBuf::from(positional)),
        }
    }
    if inputs.is_empty() {
        bail!("no input file");
    }
    if dump {
        return Ok(Command::DumpTree {
            inputs,
            metadata,
            reading,
        });
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

/// Whether the extension says the file is markdown. Markdown is what
/// the binary reads, and a name it does not recognise is a question
/// rather than a guess.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_ascii_lowercase();
            ext == "md" || ext == "markdown"
        })
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
        report_warnings(&self.warnings);
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

/// Everything the reading had to complain about, one per line.
fn report_warnings(warnings: &[Warning]) {
    for warning in warnings {
        match &warning.origin {
            Some(origin) => eprintln!("fleuron: warning: {origin}: {}", warning.message),
            None => eprintln!("fleuron: warning: {}", warning.message),
        }
    }
}

fn render(
    inputs: &[PathBuf],
    output: &Path,
    css: &[PathBuf],
    metadata: Metadata,
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
    let bytes = fleuron::pdf::write_with_assets(&laid_out, &registry, &assets, &book.metadata)
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
/// The book's metadata is what the command line named. A lone markdown
/// input is the whole book, so its frontmatter fills whatever the
/// command line left unset. Several inputs are chapters: each file's
/// frontmatter belongs to the section it became, and the book is left
/// unnamed rather than named after whichever chapter came first.
fn read_book(
    inputs: &[PathBuf],
    named: Metadata,
    reading: &Options,
) -> Result<(Book, Vec<Warning>)> {
    let mut sources = Vec::with_capacity(inputs.len());
    for input in inputs {
        if !is_markdown(input) {
            bail!("{}: not markdown", input.display());
        }
        let text =
            std::fs::read_to_string(input).with_context(|| format!("{}", input.display()))?;
        sources.push((input.display().to_string(), text));
    }

    let mut metadata = match sources.as_slice() {
        [(_, text)] => fleuron_markdown::frontmatter(text),
        _ => Metadata::default(),
    };
    // What the command line said outranks what a file said.
    metadata.title = named.title.or(metadata.title);
    metadata.author = named.author.or(metadata.author);
    metadata.extra.extend(named.extra);

    let mut sections = Vec::new();
    let mut warnings = Vec::new();
    for (source, text) in &sources {
        let (read, complaints) = fleuron_markdown::to_sections(text, source, reading);
        sections.extend(read);
        warnings.extend(complaints);
    }
    Ok((fleuron_markdown::assemble(metadata, sections), warnings))
}

/// Writes the tree the frontend read to stdout, so what a manuscript
/// became is readable without a PDF in between.
fn dump_tree(inputs: &[PathBuf], named: Metadata, reading: &Options) -> Result<Vec<Warning>> {
    let (book, warnings) = read_book(inputs, named, reading)?;
    let tree = serde_json::to_string_pretty(&book).context("the content tree serializes")?;
    println!("{tree}");
    Ok(warnings)
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
                metadata: Metadata::default(),
                reading: Options::default(),
            },
        );
        assert_eq!(
            render_of(&["--output", "out.pdf", "manuscript.markdown"]),
            Command::Render {
                inputs: vec![PathBuf::from("manuscript.markdown")],
                output: PathBuf::from("out.pdf"),
                css: Vec::new(),
                metadata: Metadata::default(),
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
                metadata: Metadata::default(),
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

    /// Markdown composes in the order the command line gives it.
    #[test]
    fn several_markdown_files_compose_in_argument_order() {
        let Command::Render { inputs, .. } = render_of(&["two.md", "one.md", "-o", "out.pdf"])
        else {
            panic!("expected a render");
        };
        assert_eq!(inputs, [PathBuf::from("two.md"), PathBuf::from("one.md")]);
    }

    #[test]
    fn metadata_fields_are_named_on_the_command_line() {
        let Command::Render { metadata, .. } = render_of(&[
            "book.md",
            "-o",
            "out.pdf",
            "--title",
            "The Levant Papers",
            "--author",
            "E. Marsh",
            "--meta",
            "language=en",
        ]) else {
            panic!("expected a render");
        };
        assert_eq!(metadata.title.as_deref(), Some("The Levant Papers"));
        assert_eq!(metadata.author.as_deref(), Some("E. Marsh"));
        assert_eq!(
            metadata.extra.get("language").map(String::as_str),
            Some("en"),
        );

        for bad in [
            vec!["book.md", "-o", "out.pdf", "--meta", "language"],
            vec!["book.md", "-o", "out.pdf", "--title"],
        ] {
            assert!(parse(args(&bad)).is_err(), "{bad:?}");
        }
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
    /// and do not, so the command line has to.
    #[test]
    fn book_metadata_comes_from_one_file_or_from_the_command_line() {
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
        let (book, _) = read_book(std::slice::from_ref(&one), Metadata::default(), &whole).unwrap();
        assert_eq!(book.metadata.title.as_deref(), Some("The Ambassador"));

        // Several: the book is unnamed, and each chapter keeps its own
        // title rather than lending it to the work.
        let (book, _) =
            read_book(&[one.clone(), two.clone()], Metadata::default(), &whole).unwrap();
        assert_eq!(book.metadata.title, None);
        let chapters: Vec<Option<&str>> = book
            .sections
            .iter()
            .map(|section| section.title.as_deref())
            .collect();
        assert_eq!(chapters, [Some("The Ambassador"), Some("A Cold Reception")]);

        // Named on the command line, the work has a title and the
        // chapters keep theirs.
        let named = Metadata {
            title: Some("The Levant Papers".into()),
            author: Some("E. Marsh".into()),
            extra: [("language".to_string(), "en".to_string())]
                .into_iter()
                .collect(),
        };
        let (book, _) = read_book(&[one.clone(), two], named.clone(), &whole).unwrap();
        assert_eq!(book.metadata, named);
        assert_eq!(book.sections[0].title.as_deref(), Some("The Ambassador"));

        // A flag outranks the frontmatter it overlaps.
        let (book, _) = read_book(std::slice::from_ref(&one), named, &whole).unwrap();
        assert_eq!(book.metadata.title.as_deref(), Some("The Levant Papers"));
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
        let e = read_book(&missing, Metadata::default(), &Options::default()).unwrap_err();
        assert!(format!("{e:#}").contains("no/such/book.md"), "{e:#}");
    }

    /// Markdown is what the binary reads, and a file it cannot place
    /// is named rather than guessed at.
    #[test]
    fn an_input_that_is_not_markdown_is_an_error_not_a_panic() {
        let e = read_book(
            &[PathBuf::from("notes.txt")],
            Metadata::default(),
            &Options::default(),
        )
        .unwrap_err();
        assert!(format!("{e:#}").contains("notes.txt"), "{e:#}");
        assert!(
            read_book(
                &[PathBuf::from("book.json")],
                Metadata::default(),
                &Options::default(),
            )
            .is_err()
        );
    }

    /// The tree is a job of its own: it needs no output path, and two
    /// dumps of one manuscript are the same bytes.
    #[test]
    fn dump_tree_is_a_job_of_its_own() {
        assert_eq!(
            parse(args(&["book.md", "--dump-tree"])).unwrap(),
            Command::DumpTree {
                inputs: vec![PathBuf::from("book.md")],
                metadata: Metadata::default(),
                reading: Options::default(),
            },
        );

        let dir = std::env::temp_dir().join("fleuron-cli-dump");
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let source = dir.join("dump.md");
        std::fs::write(
            &source,
            "---\ntitle: The Levant Papers\n---\n\n# One\n\nHe arrived on a Tuesday.\n",
        )
        .expect("the source is writable");

        let read = || {
            let (book, warnings) = read_book(
                std::slice::from_ref(&source),
                Metadata::default(),
                &Options::default(),
            )
            .unwrap();
            assert!(warnings.is_empty());
            serde_json::to_string_pretty(&book).unwrap()
        };
        let tree = read();
        assert!(tree.contains("The Levant Papers"));
        assert!(tree.contains("\"type\": \"heading\""));
        assert!(!tree.contains("\"id\""), "ids do not travel");
        assert_eq!(tree, read(), "the dump moved between runs");
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
