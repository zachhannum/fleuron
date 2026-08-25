fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("version") | Some("--version") => {
            println!("fleuron {}", env!("CARGO_PKG_VERSION"));
        }
        _ => {
            eprintln!("fleuron: pre-alpha — no commands implemented yet.");
            eprintln!("usage: fleuron <input> [-o <output.pdf>]");
            std::process::exit(2);
        }
    }
}
