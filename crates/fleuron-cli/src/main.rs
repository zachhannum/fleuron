use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("version") | Some("--version") => {
            println!("fleuron {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("-h") | None => {
            eprintln!("usage: fleuron <input> [-o <output.pdf>]");
            ExitCode::from(2)
        }
        Some(input) => run(input, flag_value(&args, "-o").map(PathBuf::from)),
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn run(input: &str, output: Option<PathBuf>) -> ExitCode {
    // Placeholder pipeline entry: proves the e2e path (input exists,
    // is readable, is a valid content tree, fonts load and shape) and
    // reports the honest state — layout is unimplemented. Exit 2 is
    // the contract for "parsed, stages pending".
    let registry = match fleuron::fonts::bundled_registry() {
        Ok(registry) => registry,
        Err(e) => {
            eprintln!("fleuron: bundled font failed to load: {e}");
            return ExitCode::FAILURE;
        }
    };
    match std::fs::read_to_string(input) {
        Ok(text) => match serde_json::from_str::<fleuron::content::Book>(&text) {
            Ok(mut book) => {
                book.assign_node_ids();
                let sections = book.sections.len();
                let blocks: usize = book.sections.iter().map(|s| s.blocks.len()).sum();
                let body = registry
                    .generic(fleuron::fonts::GenericFamily::Serif)
                    .expect("bundled registry maps serif");
                let fonts = registry.len();
                let shaped = registry
                    .shape(body, &book.metadata.title.clone().unwrap_or_default())
                    .map(|glyphs| glyphs.len())
                    .unwrap_or(0);
                eprintln!(
                    "fleuron: parsed {} ({} sections, {} blocks); {} font(s) registered, serif shapes title to {} glyphs; pipeline stages not yet implemented",
                    input, sections, blocks, fonts, shaped
                );
                let _ = output; // the PDF writer consumes this
                ExitCode::from(2)
            }
            Err(e) => {
                eprintln!("fleuron: {input}: invalid content tree: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("fleuron: {input}: {e}");
            ExitCode::FAILURE
        }
    }
}
