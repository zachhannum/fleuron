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

use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::metadata::{DateTime, Metadata as PdfMetadata};
use krilla::page::PageSettings;
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph, Tag};
use krilla::{Document, SerializeSettings};

use crate::LayoutOutput;
use crate::content::Metadata;
use crate::fonts::FontRegistry;
use crate::images::Assets;
use crate::pages::{DrawItem, Glyph, Page};

/// What can go wrong turning a display list into a PDF.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    /// The face named could not be embedded.
    #[error("font {0} could not be embedded")]
    Font(String),
    /// The image named is in no format the writer can embed.
    #[error("image {0} could not be embedded")]
    Image(String),
    /// A draw item falls outside what PDF can express.
    #[error("page {number} draws {kind} outside what PDF can express")]
    Geometry {
        /// The folio the offending item is on.
        number: u32,
        /// What kind of item it was.
        kind: &'static str,
    },
    /// The writer refused the document.
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
    write_with_assets(output, registry, crate::layout::no_assets(), metadata)
}

/// The same, over the images the layout pass sized.
///
/// Images resolve as fonts do: the display list carries indexes, the
/// asset table owns the files, and what reaches the page is the file
/// the header was read from.
pub fn write_with_assets(
    output: &LayoutOutput,
    registry: &FontRegistry,
    assets: &Assets,
    metadata: &Metadata,
) -> Result<Vec<u8>, PdfError> {
    write_with(
        output,
        registry,
        assets,
        metadata,
        SerializeSettings::default(),
    )
}

fn write_with(
    output: &LayoutOutput,
    registry: &FontRegistry,
    assets: &Assets,
    metadata: &Metadata,
    settings: SerializeSettings,
) -> Result<Vec<u8>, PdfError> {
    let fonts = embed_fonts(registry)?;
    let images = embed_images(assets)?;
    let mut document = Document::new_with(settings);
    document.set_metadata(document_metadata(metadata));
    for page in &output.pages {
        let mut pdf_page = document.start_page_with(PageSettings::new(page.width, page.height));
        let mut surface = pdf_page.surface();
        for item in &page.items {
            paint(&mut surface, item, page, &fonts, &images, registry)?;
        }
        surface.finish();
        pdf_page.finish();
    }
    document
        .finish()
        .map_err(|e| PdfError::Serialize(format!("{e:?}")))
}

/// Every asset as a krilla image, indexed as the display list
/// indexes them.
///
/// The format is read off the bytes rather than off the url, because
/// the url is the host's and means whatever the host says it means.
/// PDF's `DCTDecode` is the JPEG stream itself, so a JPEG travels
/// into the file as it arrived; the raster formats are decoded once,
/// alpha channel and all.
fn embed_images(assets: &Assets) -> Result<Vec<Image>, PdfError> {
    assets
        .assets()
        .iter()
        .enumerate()
        .map(|(index, asset)| {
            let bytes = assets
                .bytes(index as u32)
                .ok_or_else(|| PdfError::Image(asset.url.clone()))?
                .to_vec();
            embed_image(bytes).ok_or_else(|| PdfError::Image(asset.url.clone()))
        })
        .collect()
}

/// One image, in whichever format its own header says it is.
///
/// Interpolation is off, because an image placed at the size its own
/// header asked for is already at the resolution it was made for.
fn embed_image(bytes: Vec<u8>) -> Option<Image> {
    let data = krilla::Data::from(bytes);
    match () {
        _ if data.as_ref().starts_with(b"\x89PNG\r\n\x1a\n") => Image::from_png(data, false),
        _ if data.as_ref().starts_with(&[0xFF, 0xD8]) => Image::from_jpeg(data, false),
        _ if data.as_ref().starts_with(b"GIF8") => Image::from_gif(data, false),
        _ if data.as_ref().starts_with(b"RIFF") => Image::from_webp(data, false),
        _ => None,
    }
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
///
/// The creation date is the book's own `date`, because the engine
/// reads no clock: a run that stamped the hour would write different
/// bytes every time it ran, and two runs over one book are meant to
/// be one file.
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
    if let Some(date) = metadata.extra.get("date").and_then(|date| written(date)) {
        pdf = pdf.creation_date(date);
    }
    pdf
}

/// A date as a book writes one: `2026-08-27`, and as much of
/// `T14:30:00` after it as the book bothered with. Anything else is
/// no date and no creation date.
fn written(date: &str) -> Option<DateTime> {
    let number = |field: Option<&str>, digits: usize| -> Option<u16> {
        let field = field?;
        (field.len() == digits && field.bytes().all(|b| b.is_ascii_digit()))
            .then(|| field.parse().ok())?
    };
    let (day, clock) = match date.split_once(['T', ' ']) {
        Some((day, clock)) => (day, Some(clock)),
        None => (date, None),
    };
    let mut fields = day.split('-');
    let mut stamp = DateTime::new(number(fields.next(), 4)?);
    if let Some(month) = number(fields.next(), 2) {
        stamp = stamp.month(month as u8);
        if let Some(day) = number(fields.next(), 2) {
            stamp = stamp.day(day as u8);
        }
    }
    let mut fields = clock.unwrap_or_default().split(':');
    if let Some(hour) = number(fields.next(), 2) {
        stamp = stamp.hour(hour as u8);
        if let Some(minute) = number(fields.next(), 2) {
            stamp = stamp.minute(minute as u8);
            if let Some(second) = number(fields.next(), 2) {
                stamp = stamp.second(second as u8);
            }
        }
    }
    Some(stamp)
}

fn paint(
    surface: &mut Surface,
    item: &DrawItem,
    page: &Page,
    fonts: &[Font],
    images: &[Image],
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
        // The transform is layout's decision, not the writer's: an
        // image krilla draws into a unit square is translated to the
        // corner layout put it at and scaled to the box layout sized
        // for it.
        DrawItem::Image { x, y, w, h, asset } => {
            let image = images
                .get(*asset as usize)
                .ok_or_else(|| PdfError::Image(asset.to_string()))?;
            let size = Size::from_wh(*w, *h).ok_or(PdfError::Geometry {
                number: page.number,
                kind: "an image",
            })?;
            surface.push_transform(&Transform::from_translate(*x, *y));
            surface.draw_image(image.clone(), size);
            surface.pop();
        }
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

    /// The fixture plate: a JPEG at 300dpi, so its intrinsic size is
    /// not its pixel count.
    const PLATE: &[u8] = include_bytes!("../../../fixtures/images/plate.jpg");

    /// The fixture ornament: a PNG whose ground is transparent, and
    /// the same image as lossless WebP.
    const ORNAMENT: &[u8] = include_bytes!("../../../fixtures/images/fleuron.png");
    const ORNAMENT_WEBP: &[u8] = include_bytes!("../../../fixtures/images/fleuron.webp");

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
                extra: [("date".to_string(), "1726-10-28".to_string())]
                    .into_iter()
                    .collect(),
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
            assets: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// An asset table over files a host handed over by name.
    fn assets(files: &[(&str, &[u8])]) -> Assets {
        let mut assets = Assets::none();
        for (url, bytes) in files {
            assets.add(url, bytes.to_vec()).expect("the header reads");
        }
        assets
    }

    /// PDF bytes as inspectable text: content streams uncompressed,
    /// bytes read as latin-1 so offsets survive.
    fn readable(output: &LayoutOutput, metadata: &Metadata) -> String {
        with_images(output, &Assets::none(), metadata)
    }

    /// The same, over images the host supplied.
    fn with_images(output: &LayoutOutput, assets: &Assets, metadata: &Metadata) -> String {
        latin1(&bytes_of(output, assets, metadata))
    }

    fn bytes_of(output: &LayoutOutput, assets: &Assets, metadata: &Metadata) -> Vec<u8> {
        write_with(
            output,
            registry(),
            assets,
            metadata,
            SerializeSettings {
                compress_content_streams: false,
                ..Default::default()
            },
        )
        .expect("the bundled face embeds")
    }

    fn latin1(bytes: &[u8]) -> String {
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

    /// A JPEG is embedded as it arrived: PDF's `DCTDecode` is the
    /// JPEG stream itself, so re-encoding one would cost quality for
    /// nothing.
    #[test]
    fn a_jpeg_embeds_the_bytes_it_arrived_as() {
        let items = vec![DrawItem::Image {
            x: 40.0,
            y: 60.0,
            w: 115.2,
            h: 76.8,
            asset: 0,
        }];
        let table = assets(&[("plate.jpg", PLATE)]);
        let bytes = bytes_of(&page_of(items, 432.0, 648.0), &table, &Metadata::default());
        let pdf = latin1(&bytes);
        assert!(pdf.contains("/DCTDecode"), "the plate was re-encoded");
        assert!(
            bytes.windows(PLATE.len()).any(|window| window == PLATE),
            "the embedded stream is not the file that went in",
        );
    }

    /// The raster formats decode, and an alpha channel becomes the
    /// image's soft mask, which is what lets the paper, or a rule
    /// under it, show through.
    #[test]
    fn a_raster_image_with_alpha_embeds_a_soft_mask() {
        for (url, bytes) in [("fleuron.png", ORNAMENT), ("fleuron.webp", ORNAMENT_WEBP)] {
            let items = vec![
                DrawItem::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 432.0,
                    h: 648.0,
                },
                DrawItem::Image {
                    x: 200.0,
                    y: 300.0,
                    w: 30.72,
                    h: 30.72,
                    asset: 0,
                },
            ];
            let table = assets(&[(url, bytes)]);
            let pdf = with_images(&page_of(items, 432.0, 648.0), &table, &Metadata::default());
            assert!(
                pdf.contains("/SMask"),
                "{url}: the ornament's transparency was flattened away",
            );
            assert!(
                pdf.contains("/ColorSpace /DeviceGray"),
                "{url}: no grayscale mask stream",
            );
            assert!(
                !pdf.contains("/DCTDecode"),
                "{url}: a raster image went in as a JPEG stream",
            );
        }
    }

    /// Painters scale; they do not re-derive. An image layout sized
    /// down draws at the size layout gave it, not at the one its
    /// header declares.
    #[test]
    fn an_image_draws_at_the_size_layout_gave_it() {
        let items = vec![DrawItem::Image {
            x: 54.0,
            y: 90.0,
            w: 72.0,
            h: 48.0,
            asset: 0,
        }];
        let table = assets(&[("plate.jpg", PLATE)]);
        let (width, height) = table
            .lookup("plate.jpg")
            .expect("the plate is an asset")
            .1
            .size();
        assert!(
            (width - 115.2).abs() < 0.01 && (height - 76.8).abs() < 0.01,
            "the header sizes the plate at {width}x{height}pt",
        );
        let pdf = with_images(&page_of(items, 432.0, 648.0), &table, &Metadata::default());
        // krilla draws an image into the unit square, so the box is
        // the transform: the size layout asked for, and the corner it
        // asked for, measured up from the foot of the page.
        assert!(
            pdf.contains("72 0 0 48 54 510 cm"),
            "the plate is not placed at 72x48pt at (54, 90):\n{pdf}",
        );
    }

    /// An image nothing supplied is no draw item, and one warning
    /// naming the url.
    #[test]
    fn an_image_no_host_supplied_is_reported_and_skipped() {
        let mut book = Book {
            sections: vec![Section {
                blocks: vec![Block::Image {
                    id: Default::default(),
                    url: "missing.png".into(),
                    alt: "a plate".into(),
                    position: None,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        book.assign_node_ids();
        let output = laid_out(&book);
        assert!(
            !output.pages.iter().any(|page| page
                .items
                .iter()
                .any(|item| matches!(item, DrawItem::Image { .. }))),
            "an image with no bytes was placed anyway",
        );
        assert_eq!(output.warnings.len(), 1, "{:?}", output.warnings);
        assert!(output.warnings[0].message.contains("missing.png"));
    }

    /// Book metadata reaches the document information dictionary.
    #[test]
    fn metadata_carries_the_book_title() {
        let book = book("Some prose.");
        let pdf = readable(&laid_out(&book), &book.metadata);
        assert!(pdf.contains("/Title (Gulliver's Travels)"), "no title");
        assert!(pdf.contains("/Author (Jonathan Swift)"), "no author");
        assert!(pdf.contains("/Producer (fleuron)"), "no producer");
        assert!(
            pdf.contains("/CreationDate (D:17261028"),
            "the book's own date is not the creation date:\n{pdf}",
        );
    }

    /// A date the book does not write is no creation date, rather
    /// than the hour the run happened to start: the engine reads no
    /// clock, and two runs over one book are one file.
    #[test]
    fn a_book_with_no_date_is_stamped_with_none() {
        let mut book = book("Some prose.");
        book.metadata.extra.remove("date");
        let pdf = readable(&laid_out(&book), &book.metadata);
        assert!(!pdf.contains("/CreationDate"), "a clock was read");
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
