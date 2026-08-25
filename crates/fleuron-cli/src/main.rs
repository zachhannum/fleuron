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
    // Pipeline entry: content tree in, paginated display list out.
    // Exit 2 is the no-PDF-writer contract.
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
                let result = fleuron::layout::layout_book(&book, &registry);
                let warnings = result.warnings.len();
                eprintln!(
                    "fleuron: {} — {} pages, {} warning{}",
                    input,
                    result.pages.len(),
                    warnings,
                    if warnings == 1 { "" } else { "s" },
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
