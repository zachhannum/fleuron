//! Fixtures and measurement: the harness the engine is measured
//! against.
//!
//! Two real books, checked in as markdown and read into content trees
//! through the shipped frontend; criterion benches that time one
//! pipeline stage at a time; and a gate binary that runs a whole book
//! against absolute budgets, the same way natively and under wasm.
//!
//! Nothing here ships. The crate exists so that a perf claim about
//! fleuron is a number somebody can reproduce.

#![deny(missing_docs)]

pub mod alloc;
pub mod corpus;
pub mod gate;

pub use corpus::Corpus;

/// The bundled font registry, shared across benches and gate runs so
/// that font parsing is not counted as layout.
pub fn registry() -> &'static fleuron::fonts::FontRegistry {
    static REGISTRY: std::sync::OnceLock<fleuron::fonts::FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| fleuron::fonts::bundled_registry().expect("bundled font parses"))
}

/// One book's styling under the built-in sheet alone, compiled
/// against the shared registry. The default sheet loads no author
/// fonts, so nothing is added to it.
pub fn styles(book: &fleuron::content::Book) -> fleuron::style::StyleTree {
    fleuron::style::defaults(book, registry())
}

/// Every block of a book as the flat text one shaping call is handed,
/// in document order. v0.1 puts one face on everything, so a block is
/// a run and a run is a shaping call.
pub fn shaped_texts(book: &fleuron::content::Book) -> Vec<String> {
    use fleuron::content::Block;
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    let text = fleuron::content::text(inlines);
                    if !text.is_empty() {
                        out.push(text);
                    }
                }
                Block::Blockquote { blocks, .. } => walk(blocks, out),
                Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }
    let mut out = Vec::new();
    for section in &book.sections {
        walk(&section.blocks, &mut out);
    }
    out
}
