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
pub mod anchored_images;
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

/// The same on the page box `division` asks for, with whatever
/// `illustration` anchors to it. The undivided, bare pair is the
/// built-in sheet alone.
pub fn styles_on(
    book: &fleuron::content::Book,
    division: gate::Division,
    illustration: gate::Illustration,
) -> fleuron::style::StyleTree {
    let css = format!("{}{}", division.css(), illustration.css());
    if css.is_empty() {
        return styles(book);
    }
    fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author("gate.css", &css)])
        .compile(book, registry())
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
                Block::Table { head, body, .. } => {
                    for blocks in fleuron::content::cell_blocks(head, body) {
                        walk(blocks, out);
                    }
                }
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

/// The box every image on one page covers, as `[left, top, right,
/// bottom]` in page coordinates.
pub fn image_boxes(page: &fleuron::pages::Page) -> Vec<[f32; 4]> {
    use fleuron::pages::DrawItem;
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Image { x, y, w, h, .. } => Some([*x, *y, *x + *w, *y + *h]),
            _ => None,
        })
        .collect()
}

/// The box every text run on one page covers, in the same form.
///
/// A run's height is the face's own ascender and descender at the
/// size it was set in, which is the space a line of it claims.
pub fn run_boxes(page: &fleuron::pages::Page) -> Vec<[f32; 4]> {
    use fleuron::pages::DrawItem;
    page.items
        .iter()
        .filter_map(|item| {
            let DrawItem::Text {
                x,
                y,
                font_id,
                size,
                glyphs,
                ..
            } = item
            else {
                return None;
            };
            let metrics = registry().metrics(*font_id)?;
            let last = glyphs.last()?;
            let upem = metrics.units_per_em as f32;
            let advance = registry().advance_width(*font_id, last.id).unwrap_or(0) as f32;
            Some([
                *x,
                y - metrics.ascender as f32 / upem * size,
                last.x + advance / upem * size,
                y - metrics.descender as f32 / upem * size,
            ])
        })
        .collect()
}
