//! PDF export: display list in, PDF bytes out.
//!
//! The painter for print. Everything it needs already exists on the
//! page — glyph ids, absolute positions, page trim — so this stage
//! places what layout decided and decides nothing itself.
//!
//! Glyphs are shown at their own x, not at the font's: kerning and
//! justification put them where they are, and a PDF that re-derived
//! positions from advances would disagree with the preview. The text
//! each run was shaped from travels with it, so the writer can build
//! the glyph-to-character map that makes the text selectable.

use krilla::geom::{PathBuilder, Point, Rect};
use krilla::metadata::Metadata as PdfMetadata;
use krilla::page::PageSettings;
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph, Tag};
use krilla::{Document, SerializeSettings};

use crate::LayoutOutput;
use crate::content::Metadata;
use crate::fonts::FontRegistry;
use crate::pages::{DrawItem, Glyph, Page};

/// What can go wrong turning a display list into a PDF.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("font {0} could not be embedded")]
    Font(String),
    #[error("page {number} draws {kind} outside what PDF can express")]
    Geometry { number: u32, kind: &'static str },
    #[error("PDF serialization failed: {0}")]
    Serialize(String),
}

/// Writes one laid-out book as PDF bytes.
///
/// Fonts resolve through the registry that shaped the run: the
/// display list carries ids, the registry owns the bytes, and the
/// embedded subset therefore holds the same outlines the shaper
/// measured.
pub fn write(
    output: &LayoutOutput,
    registry: &FontRegistry,
    metadata: &Metadata,
) -> Result<Vec<u8>, PdfError> {
    write_with(output, registry, metadata, SerializeSettings::default())
}

fn write_with(
    output: &LayoutOutput,
    registry: &FontRegistry,
    metadata: &Metadata,
    settings: SerializeSettings,
) -> Result<Vec<u8>, PdfError> {
    let fonts = embed_fonts(registry)?;
    let mut document = Document::new_with(settings);
    document.set_metadata(document_metadata(metadata));
    for page in &output.pages {
        let mut pdf_page = document.start_page_with(PageSettings::new(page.width, page.height));
        let mut surface = pdf_page.surface();
        for item in &page.items {
            paint(&mut surface, item, page, &fonts, registry)?;
        }
        surface.finish();
        pdf_page.finish();
    }
    document
        .finish()
        .map_err(|e| PdfError::Serialize(format!("{e:?}")))
}

/// Every registered face as a krilla font, indexed by `font_id`.
/// krilla subsets on write, so a face nothing draws with costs
/// nothing.
///
/// A face off its family's default location embeds as that instance:
/// the outlines a reader gets are the ones the shaper measured, not
/// the whole axis.
fn embed_fonts(registry: &FontRegistry) -> Result<Vec<Font>, PdfError> {
    (0..registry.len() as u16)
        .map(|id| {
            let name = || {
                registry
                    .font_ref(id)
                    .map(|f| f.name.clone())
                    .unwrap_or_else(|| id.to_string())
            };
            let bytes = registry.bytes(id).ok_or_else(|| PdfError::Font(name()))?;
            let variations: Vec<(Tag, f32)> = registry
                .variations(id)
                .unwrap_or_default()
                .iter()
                .map(|axis| (Tag::new(&axis.tag), axis.value))
                .collect();
            Font::new_variable(bytes.into(), 0, &variations).ok_or_else(|| PdfError::Font(name()))
        })
        .collect()
}

/// Book metadata as document metadata. `fleuron` is both creator and
/// producer: the engine laid the pages out and wrote the file.
fn document_metadata(metadata: &Metadata) -> PdfMetadata {
    let mut pdf = PdfMetadata::new()
        .creator("fleuron".to_string())
        .producer("fleuron".to_string());
    if let Some(title) = &metadata.title {
        pdf = pdf.title(title.clone());
    }
    if let Some(author) = &metadata.author {
        pdf = pdf.authors(vec![author.clone()]);
    }
    if let Some(language) = metadata.extra.get("language") {
        pdf = pdf.language(language.clone());
    }
    pdf
}

fn paint(
    surface: &mut Surface,
    item: &DrawItem,
    page: &Page,
    fonts: &[Font],
    registry: &FontRegistry,
) -> Result<(), PdfError> {
    match item {
        DrawItem::Text {
            x,
            y,
            font_id,
            size,
            text,
            glyphs,
        } => {
            let font = fonts
                .get(*font_id as usize)
                .ok_or_else(|| PdfError::Font(font_id.to_string()))?;
            let placed = place(glyphs, text, *x, *size, *font_id, registry);
            surface.draw_glyphs(
                Point::from_xy(*x, *y),
                &placed,
                font.clone(),
                text,
                *size,
                false,
            );
        }
        DrawItem::Rect { x, y, w, h } => {
            let rect = Rect::from_xywh(*x, *y, *w, *h).ok_or(PdfError::Geometry {
                number: page.number,
                kind: "a rectangle",
            })?;
            let mut builder = PathBuilder::new();
            builder.push_rect(rect);
            let path = builder.finish().ok_or(PdfError::Geometry {
                number: page.number,
                kind: "a rectangle",
            })?;
            surface.draw_path(&path);
        }
        // Images arrive with the image pipeline; layout emits none.
        DrawItem::Image { .. } => {}
    }
    Ok(())
}

/// Display-list glyphs as krilla glyphs.
///
/// krilla walks a run by advances from its origin, so an absolute x
/// becomes the gap to the glyph after it. The last glyph has no
/// successor and takes the advance its font gives it — anything else
/// would leave a spurious adjustment at the end of every run.
/// Advances are in ems, hence the division by size.
fn place(
    glyphs: &[Glyph],
    text: &str,
    origin: f32,
    size: f32,
    font_id: u16,
    registry: &FontRegistry,
) -> Vec<KrillaGlyph> {
    let upem = registry
        .metrics(font_id)
        .map(|m| m.units_per_em as f32)
        .unwrap_or(1000.0);
    // The run's origin need not be its first glyph's x; the gap
    // shifts every glyph, since krilla measures the cursor from the
    // origin.
    let offset = glyphs.first().map(|g| g.x - origin).unwrap_or(0.0);
    glyphs
        .iter()
        .enumerate()
        .map(|(i, glyph)| {
            let advance = match glyphs.get(i + 1) {
                Some(next) => (next.x - glyph.x) / size,
                None => registry.advance_width(font_id, glyph.id).unwrap_or(0) as f32 / upem,
            };
            KrillaGlyph {
                glyph_id: GlyphId::new(glyph.id),
                text_range: clamp_range(glyph, text),
                x_advance: advance,
                x_offset: offset / size,
                y_offset: 0.0,
                y_advance: 0.0,
                location: None,
            }
        })
        .collect()
}

/// A glyph's range, kept inside the run's text and on character
/// boundaries: krilla slices the text with it to build the ToUnicode
/// map, and a bad range there is a panic, not a wrong glyph.
fn clamp_range(glyph: &Glyph, text: &str) -> std::ops::Range<usize> {
    let floor = |at: u32| {
        let mut at = (at as usize).min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let start = floor(glyph.range.start);
    start..floor(glyph.range.end).max(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Block, Book, Inline, Section};
    use crate::fonts::{BUNDLED_FONT, bundled_registry};
    use crate::pages::{Glyph, Side};

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
    }

    /// A book of one paragraph, with title metadata.
    fn book(text: &str) -> Book {
        let mut book = Book {
            metadata: Metadata {
                title: Some("Gulliver\'s Travels".into()),
                author: Some("Jonathan Swift".into()),
                ..Default::default()
            },
            sections: vec![Section {
                blocks: vec![Block::Paragraph {
                    id: Default::default(),
                    inlines: vec![Inline::Text {
                        id: Default::default(),
                        value: text.into(),
                        position: None,
                    }],
                    position: None,
                }],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        book
    }

    /// One hand-built page: the display list a painter sees, without
    /// going through layout.
    fn page_of(items: Vec<DrawItem>, width: f32, height: f32) -> LayoutOutput {
        LayoutOutput {
            pages: vec![Page {
                number: 1,
                side: Side::Recto,
                width,
                height,
                items,
            }],
            fonts: registry().font_ref(0).cloned().into_iter().collect(),
            warnings: Vec::new(),
        }
    }

    /// PDF bytes as inspectable text: content streams uncompressed,
    /// bytes read as latin-1 so offsets survive.
    fn readable(output: &LayoutOutput, metadata: &Metadata) -> String {
        let bytes = write_with(
            output,
            registry(),
            metadata,
            SerializeSettings {
                compress_content_streams: false,
                ..Default::default()
            },
        )
        .expect("the bundled face embeds");
        bytes.iter().map(|b| *b as char).collect()
    }

    fn laid_out(book: &Book) -> LayoutOutput {
        let styles = crate::style::defaults(book, registry());
        crate::layout::layout_book(book, &styles, registry())
    }

    /// The one glyph-positioning offset inside a showing op, in
    /// 1000ths of an em: `[(…) 100 (…)] TJ`.
    fn showing_offset(pdf: &str) -> Option<f32> {
        let show = &pdf[pdf.find("[(")?..pdf.find("] TJ")?];
        show.rsplit_once(") ")?.1.split_once(" (")?.0.parse().ok()
    }

    /// The page's trim size is the media box: krilla is told the size
    /// the display list carries, not a default.
    #[test]
    fn page_trim_becomes_the_media_box() {
        let pdf = readable(&page_of(Vec::new(), 432.0, 648.0), &Metadata::default());
        assert!(
            pdf.contains("/MediaBox [0 0 432 648]"),
            "media box missing from:\n{pdf}"
        );
    }

    /// Glyphs show where layout put them. A run whose second glyph
    /// sits a hair off its font advance must come out as one showing
    /// op carrying that offset — 1000ths of an em, positive leftwards
    /// — not as two runs and not as font-advance spacing.
    #[test]
    fn glyphs_show_at_the_positions_layout_gave_them() {
        let d = registry().char_glyph(0, 'd').unwrap();
        let advance = registry().advance_width(0, d).unwrap() as f32 / 1000.0 * 10.0;
        let items = vec![DrawItem::Text {
            x: 0.0,
            y: 20.0,
            font_id: 0,
            size: 10.0,
            text: "dd".into(),
            glyphs: vec![
                Glyph {
                    id: d,
                    x: 0.0,
                    range: 0..1,
                },
                Glyph {
                    id: d,
                    x: advance - 1.0,
                    range: 1..2,
                },
            ],
        }];
        let pdf = readable(&page_of(items, 200.0, 200.0), &Metadata::default());
        // 1pt tighter than the font advance, at 10pt: 100/1000 em,
        // positive because PDF offsets pull the next glyph leftwards.
        let offset = showing_offset(&pdf).expect("no showing offset in:\n{pdf}");
        assert!(
            (offset - 100.0).abs() < 0.01,
            "pair shows {offset}/1000 em tight, layout asked for 100"
        );
    }

    /// Rules paint as filled paths at the coordinates they carry.
    #[test]
    fn rect_items_paint_as_filled_paths() {
        let items = vec![DrawItem::Rect {
            x: 54.0,
            y: 100.0,
            w: 324.0,
            h: 0.5,
        }];
        let pdf = readable(&page_of(items, 432.0, 648.0), &Metadata::default());
        assert!(
            pdf.contains("54 100 m") && pdf.contains("378 100 l") && pdf.contains("\nf\n"),
            "rect did not paint as a filled path:\n{pdf}"
        );
    }

    /// The face embeds as a subset: a tagged BaseFont, a FontFile2,
    /// and a stream far smaller than the face it came from.
    #[test]
    fn the_face_embeds_as_a_subset() {
        let pdf = readable(&laid_out(&book("difficult offices")), &Metadata::default());
        let tag = pdf
            .find("+EBGaramond-Regular")
            .expect("no subset-tagged BaseFont");
        assert!(
            pdf[tag - 6..tag].chars().all(|c| c.is_ascii_uppercase()),
            "BaseFont carries no six-letter subset tag"
        );
        assert!(pdf.contains("/FontFile2"), "no embedded font program");
        assert!(
            pdf.len() < BUNDLED_FONT.len(),
            "the whole PDF ({} bytes) is no smaller than the full face ({} bytes)",
            pdf.len(),
            BUNDLED_FONT.len()
        );
    }

    /// Book metadata reaches the document information dictionary.
    #[test]
    fn metadata_carries_the_book_title() {
        let book = book("Some prose.");
        let pdf = readable(&laid_out(&book), &book.metadata);
        assert!(pdf.contains("/Title (Gulliver's Travels)"), "no title");
        assert!(pdf.contains("/Author (Jonathan Swift)"), "no author");
    }

    /// Text is selectable because every glyph maps back to the
    /// characters it was shaped from: the ffi ligature is one glyph
    /// and three code points in the ToUnicode map.
    #[test]
    fn glyphs_map_back_to_their_characters() {
        let pdf = readable(&laid_out(&book("difficult offices")), &Metadata::default());
        assert!(pdf.contains("/ToUnicode"), "no ToUnicode map");
        assert!(
            pdf.contains("<006600660069>"),
            "the ffi ligature does not map back to f, f, i:\n{pdf}"
        );
    }

    /// The file is structurally whole — header, page tree, cross
    /// reference, trailer — which is what `qpdf --check` reads.
    #[test]
    fn the_document_is_structurally_whole() {
        let book = book("Some prose.");
        let output = laid_out(&book);
        let pdf = readable(&output, &book.metadata);
        assert!(pdf.starts_with("%PDF-1.7"), "no PDF header");
        assert!(
            pdf.contains(&format!("/Type /Pages\n  /Count {}", output.pages.len())),
            "page tree does not count the display list's pages"
        );
        assert!(pdf.contains("startxref"), "no cross-reference offset");
        assert!(pdf.trim_end().ends_with("%%EOF"), "no trailer");
    }

    /// Two runs over one book produce byte-identical PDFs: nothing in
    /// the writer reads a clock or a hash of an address.
    #[test]
    fn writing_is_deterministic() {
        let book = book("Some prose, twice over.");
        let output = laid_out(&book);
        let first = write(&output, registry(), &book.metadata).unwrap();
        let second = write(&output, registry(), &book.metadata).unwrap();
        assert_eq!(first, second);
    }
}
