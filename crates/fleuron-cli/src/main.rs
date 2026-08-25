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
    // Pipeline entry: content tree in, line layout run over every
    // paragraph. Exit 2 remains the contract while downstream stages
    // (pages, PDF) are pending (#14–#17).
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
                let layout = fleuron::lines::LineLayout::new(&registry);
                let style = fleuron::lines::ParagraphStyle {
                    font_id: registry
                        .generic(fleuron::fonts::GenericFamily::Serif)
                        .expect("bundled registry maps serif"),
                    size: 11.0,
                    line_height: 1.4,
                };
                let measure = 260.0; // 6×9 book text block, until #6
                let (paragraphs, lines) =
                    count_paragraphs_and_lines(&book, &layout, style, measure);
                eprintln!(
                    "fleuron: {} — {} paragraphs laid out to {} lines at {measure}pt measure",
                    input, paragraphs, lines
                );
                let _ = output; // the PDF writer consumes this (#16)
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

/// Runs line layout over every paragraph in the book — the e2e proof
/// that the stage handles the whole fixture, not samples.
fn count_paragraphs_and_lines(
    book: &fleuron::content::Book,
    layout: &fleuron::lines::LineLayout,
    style: fleuron::lines::ParagraphStyle,
    measure: f32,
) -> (usize, usize) {
    use fleuron::content::Block;
    use fleuron::lines::LineBreakOptions;

    let mut paragraphs = 0usize;
    let mut lines = 0usize;
    let walk = |blocks: &[Block], paragraphs: &mut usize, lines: &mut usize| {
        for block in blocks {
            if let Block::Paragraph { inlines, .. } = block {
                *paragraphs += 1;
                *lines += layout
                    .layout(inlines, style, measure, LineBreakOptions::default())
                    .len();
            }
        }
    };
    for section in &book.sections {
        walk(&section.blocks, &mut paragraphs, &mut lines);
    }
    (paragraphs, lines)
}
