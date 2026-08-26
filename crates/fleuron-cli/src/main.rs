//! The `fleuron` binary: a content tree in, a PDF out.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};

use fleuron::Warning;
use fleuron::content::Book;
use fleuron::style::{FontLoader, Source, Stylesheets};

const USAGE: &str = "usage: fleuron <input.json> -o <output.pdf> [-c <style.css>]

  -o, --output <path>  where to write the PDF
  -c, --css <path>     author stylesheet, cascading over the defaults;
                       repeatable, applied in the order given
  -V, --version        print the version and exit
  -h, --help           print this message and exit";

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
        input: PathBuf,
        output: PathBuf,
        css: Vec<PathBuf>,
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
        Ok(Command::Render { input, output, css }) => match render(&input, &output, &css) {
            Ok(summary) => summary.report(&input, &output),
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
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut css: Vec<PathBuf> = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-V" | "--version" => return Ok(Command::Version),
            "-h" | "--help" => return Ok(Command::Help),
            "-o" | "--output" => {
                let path = args.next().ok_or_else(|| anyhow!("{arg} needs a path"))?;
                output = Some(PathBuf::from(path));
            }
            "-c" | "--css" => {
                let path = args.next().ok_or_else(|| anyhow!("{arg} needs a path"))?;
                css.push(PathBuf::from(path));
            }
            flag if flag.starts_with('-') && flag.len() > 1 => bail!("unknown option {flag}"),
            positional => {
                if input.replace(PathBuf::from(positional)).is_some() {
                    bail!("one input file at a time");
                }
            }
        }
    }
    Ok(Command::Render {
        input: input.ok_or_else(|| anyhow!("no input file"))?,
        output: output.ok_or_else(|| anyhow!("no output path"))?,
        css,
    })
}

/// What one rendered book has to say for itself.
struct Summary {
    pages: usize,
    warnings: Vec<Warning>,
}

impl Summary {
    fn report(&self, input: &Path, output: &Path) -> Status {
        eprintln!(
            "fleuron: {} → {}: {} pages",
            input.display(),
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

fn render(input: &Path, output: &Path, css: &[PathBuf]) -> Result<Summary> {
    let mut registry = fleuron::fonts::bundled_registry().context("bundled font failed to load")?;
    let mut book = read_book(input)?;
    book.assign_node_ids();

    let sheets = read_sheets(css)?;
    let sources: Vec<Source> = sheets
        .iter()
        .map(|(name, text)| Source::author(name, text))
        .collect();
    let mut stylesheets = Stylesheets::parse(&sources);
    // The engine opens nothing: `@font-face` urls are resolved here,
    // against the directory of the sheet that asked for them.
    stylesheets.load_fonts(&mut registry, &Files::rooted(css));
    let styles = stylesheets.compile(&book, &registry);

    let laid_out = fleuron::layout::layout_book(&book, &styles, &registry);
    let bytes = fleuron::pdf::write(&laid_out, &registry, &book.metadata)
        .with_context(|| format!("{}", input.display()))?;
    std::fs::write(output, bytes).with_context(|| format!("{}", output.display()))?;
    Ok(Summary {
        pages: laid_out.pages.len(),
        warnings: laid_out.warnings,
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

/// The host side of `@font-face`: urls are paths, resolved against
/// the directories the stylesheets came from.
struct Files {
    roots: Vec<PathBuf>,
}

impl Files {
    fn rooted(sheets: &[PathBuf]) -> Files {
        let mut roots: Vec<PathBuf> = sheets
            .iter()
            .filter_map(|sheet| sheet.parent().map(Path::to_path_buf))
            .collect();
        roots.push(PathBuf::from("."));
        Files { roots }
    }
}

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.roots
            .iter()
            .find_map(|root| std::fs::read(root.join(url)).ok())
    }
}

fn read_book(input: &Path) -> Result<Book> {
    let text = std::fs::read_to_string(input).with_context(|| format!("{}", input.display()))?;
    parse_book(&text, input)
}

fn parse_book(text: &str, input: &Path) -> Result<Book> {
    serde_json::from_str(text).with_context(|| format!("{}: not a content tree", input.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn args_are_an_input_path_and_an_output_pdf() {
        assert_eq!(
            parse(args(&["book.json", "-o", "out.pdf"])).unwrap(),
            Command::Render {
                input: PathBuf::from("book.json"),
                output: PathBuf::from("out.pdf"),
                css: Vec::new(),
            },
        );
        assert_eq!(
            parse(args(&["--output", "out.pdf", "book.json"])).unwrap(),
            Command::Render {
                input: PathBuf::from("book.json"),
                output: PathBuf::from("out.pdf"),
                css: Vec::new(),
            },
        );
        // Stylesheets cascade in the order the command line gives them.
        assert_eq!(
            parse(args(&[
                "book.json",
                "-o",
                "out.pdf",
                "-c",
                "a.css",
                "--css",
                "b.css"
            ]))
            .unwrap(),
            Command::Render {
                input: PathBuf::from("book.json"),
                output: PathBuf::from("out.pdf"),
                css: vec![PathBuf::from("a.css"), PathBuf::from("b.css")],
            },
        );
        for incomplete in [
            vec!["-o", "out.pdf"],
            vec!["book.json"],
            vec!["book.json", "-o"],
        ] {
            assert!(parse(args(&incomplete)).is_err(), "{incomplete:?}");
        }
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
        let missing = Path::new("no/such/book.json");
        let e = read_book(missing).unwrap_err();
        assert!(format!("{e:#}").contains("no/such/book.json"), "{e:#}");
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        let e = parse_book("{ not a content tree", Path::new("book.json")).unwrap_err();
        assert!(format!("{e:#}").contains("book.json"), "{e:#}");
        assert!(parse_book("[]", Path::new("book.json")).is_err());
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
        let out = Path::new("out.pdf");
        assert_eq!(clean.report(Path::new("book.json"), out), Status::Ok);
        assert_eq!(warned.report(Path::new("book.json"), out), Status::Ok);
        assert_eq!(dispatch(args(&[])), Status::Usage);
        assert_eq!(
            dispatch(args(&["no/such/book.json", "-o", "out.pdf"])),
            Status::Failure,
        );
        assert_eq!(Status::Ok.code(), ExitCode::SUCCESS);
        assert_eq!(Status::Failure.code(), ExitCode::FAILURE);
        assert_eq!(Status::Usage.code(), ExitCode::from(2));
    }
}
