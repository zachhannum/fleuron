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
    // Placeholder pipeline entry: reads the fixture to prove the e2e
    // path (input exists, is readable, parses as JSON) and reports the
    // honest state — layout is unimplemented until the v0.1 stages land
    // (#13). The e2e job's PDF assertions activate when #16/#17 land.
    match std::fs::read_to_string(input) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(_) => {
                eprintln!(
                    "fleuron: parsed {} ({} bytes); pipeline stages not yet implemented — see #13",
                    input,
                    text.len()
                );
                let _ = output; // used from #17 onward
                ExitCode::from(2)
            }
            Err(e) => {
                eprintln!("fleuron: {input}: invalid JSON: {e}");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("fleuron: {input}: {e}");
            ExitCode::FAILURE
        }
    }
}
