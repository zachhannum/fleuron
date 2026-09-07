//! Property tests for the wire: what the engine writes, a host reads
//! back unchanged.
//!
//! The display structure crosses the boundary once per keystroke and is
//! decoded by someone else's code. So the property that matters is
//! not that the bytes parse but that nothing is lost on the way
//! through: encode, decode, encode again, and the second buffer is
//! the first one.

use fleuron::content::{NodeId, SourceRange};
use fleuron::images::{Asset, Assets, Intrinsic};
use fleuron::pages::{DrawItem, Glyph, Page, Side};
use fleuron::style::Color;
use fleuron::wire;
use fleuron::{LayoutOutput, Warning};
use proptest::prelude::*;

/// Points on a page, and sizes in them. Finite: a page whose height
/// is NaN is not a display structure the engine can produce.
fn coordinate() -> impl Strategy<Value = f32> {
    -2000.0f32..2000.0
}

fn glyph() -> impl Strategy<Value = Glyph> {
    (any::<u32>(), coordinate(), 0u32..64, 0u32..64).prop_map(|(id, x, start, len)| Glyph {
        id,
        x,
        range: start..start + len,
    })
}

/// Any colour a sheet can name, black included.
fn color() -> impl Strategy<Value = Color> {
    (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(r, g, b)| Color::rgb(r, g, b))
}

/// Where a run was written, or nothing where the engine synthesized
/// it. The nodes are the ones a parsed tree hands out.
fn origin() -> impl Strategy<Value = Option<SourceRange>> {
    let nodes = section_ids();
    proptest::option::of(
        (proptest::sample::select(nodes), 0u32..64, 0u32..64).prop_map(|(node, start, len)| {
            SourceRange {
                node,
                range: start..start + len,
            }
        }),
    )
}

fn text_item() -> impl Strategy<Value = DrawItem> {
    (
        coordinate(),
        coordinate(),
        any::<u16>(),
        1.0f32..200.0,
        ".{0,40}",
        proptest::collection::vec(glyph(), 0..12),
        color(),
        origin(),
    )
        .prop_map(
            |(x, y, font_id, size, text, glyphs, color, origin)| DrawItem::Text {
                x,
                y,
                font_id,
                size,
                // A run nothing transformed has no source of its
                // own; one that was is covered where the transform is.
                source: String::new(),
                source_map: Vec::new(),
                origin,
                features: fleuron::fonts::Features::NONE,
                color,
                text,
                glyphs,
            },
        )
}

fn item() -> impl Strategy<Value = DrawItem> {
    prop_oneof![
        text_item(),
        (
            coordinate(),
            coordinate(),
            coordinate(),
            coordinate(),
            color()
        )
            .prop_map(|(x, y, w, h, color)| DrawItem::Rect { x, y, w, h, color }),
        (
            coordinate(),
            coordinate(),
            coordinate(),
            coordinate(),
            any::<u32>()
        )
            .prop_map(|(x, y, w, h, asset)| DrawItem::Image { x, y, w, h, asset }),
    ]
}

/// Section ids as the engine hands them out. Nothing else makes a
/// `NodeId`, so the ones a page can name come from a tree that has
/// been assigned.
fn section_ids() -> Vec<NodeId> {
    let markdown = "# One\n\nProse.\n\n# Two\n\nProse.\n\n# Three\n\nProse.\n";
    let sections =
        fleuron_markdown::to_sections(markdown, "ids.md", &fleuron_markdown::Options::default()).0;
    let book = fleuron_markdown::assemble(fleuron::content::Metadata::default(), sections);
    book.sections.iter().map(|section| section.id).collect()
}

fn page() -> impl Strategy<Value = Page> {
    (
        1u32..2000,
        1.0f32..2000.0,
        1.0f32..2000.0,
        proptest::sample::subsequence(section_ids(), 0..=3),
        proptest::collection::vec(item(), 0..8),
    )
        .prop_map(|(number, width, height, sections, items)| Page {
            number,
            side: Side::of_number(number),
            width,
            height,
            sections,
            items,
        })
}

fn asset() -> impl Strategy<Value = Asset> {
    (
        "[a-z]{1,8}\\.(png|jpg)",
        1u32..4000,
        1u32..4000,
        1.0f32..600.0,
        1.0f32..600.0,
    )
        .prop_map(|(url, width, height, dpi_x, dpi_y)| Asset {
            url,
            intrinsic: Intrinsic {
                width,
                height,
                dpi_x,
                dpi_y,
            },
        })
}

fn warning() -> impl Strategy<Value = Warning> {
    (".{0,40}", proptest::option::of(".{0,20}"))
        .prop_map(|(message, origin)| Warning { message, origin })
}

/// A reply as a `LayoutOutput`, sound for re-encoding whenever the
/// reply is a whole book.
fn as_output(reply: wire::Reply) -> LayoutOutput {
    LayoutOutput {
        pages: reply.pages,
        fonts: reply.fonts,
        assets: reply.assets,
        warnings: reply.warnings,
    }
}

fn output() -> impl Strategy<Value = LayoutOutput> {
    (
        proptest::collection::vec(page(), 0..6),
        proptest::collection::vec(asset(), 0..3),
        proptest::collection::vec(warning(), 0..3),
    )
        .prop_map(|(pages, assets, warnings)| LayoutOutput {
            pages,
            fonts: Vec::new(),
            assets,
            warnings,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Encode, decode, encode: the second buffer is the first one,
    /// byte for byte, and the display structure in between is the one that
    /// went in.
    #[test]
    fn the_wire_round_trips(output in output()) {
        let bytes = wire::encode(&output).expect("a display structure encodes");
        let read = wire::decode(&bytes).expect("what the engine wrote, the engine reads");
        prop_assert_eq!(&read.pages, &output.pages);
        prop_assert_eq!(&read.fonts, &output.fonts);
        prop_assert_eq!(&read.assets, &output.assets);
        prop_assert_eq!(&read.warnings, &output.warnings);
        prop_assert_eq!(read.first, 0);
        prop_assert_eq!(read.book_pages, output.pages.len());
        prop_assert_eq!(wire::encode(&as_output(read)).expect("and encodes again"), bytes);
    }

    /// The version leads every buffer, so a host can refuse one it
    /// does not know without decoding a page of it.
    #[test]
    fn the_version_leads_every_buffer(output in output()) {
        let bytes = wire::encode(&output).expect("a display structure encodes");
        prop_assert_eq!(wire::version(&bytes).expect("the version reads"), wire::VERSION);
    }

    /// Whatever `(first, count)` a host asks for, a range decodes to
    /// exactly that slice of the book, clamped rather than out of
    /// bounds, with the tables and the book's own length intact.
    #[test]
    fn a_range_decodes_to_exactly_its_own_slice(
        output in output(),
        first in 0usize..8,
        count in 0usize..8,
    ) {
        let bytes = wire::encode_range(&output, first, count).expect("a range encodes");
        let read = wire::decode(&bytes).expect("and decodes");
        let book_pages = output.pages.len();
        let clamped_first = first.min(book_pages);
        let clamped_end = clamped_first.saturating_add(count).min(book_pages);
        prop_assert_eq!(&read.pages, &output.pages[clamped_first..clamped_end]);
        prop_assert_eq!(read.first, clamped_first);
        prop_assert_eq!(read.book_pages, book_pages);
        prop_assert_eq!(&read.fonts, &output.fonts);
        prop_assert_eq!(&read.assets, &output.assets);
        prop_assert_eq!(&read.warnings, &output.warnings);
    }
}

/// The book the engine actually produces, rather than one proptest
/// invented: real glyph positions, a real font table, real pages.
#[test]
fn a_laid_out_book_round_trips() {
    let registry = fleuron::fonts::bundled_registry().expect("bundled font parses");
    let book = fleuron_markdown::assemble(
        fleuron::content::Metadata::default(),
        fleuron_markdown::to_sections(
            "# One\n\nThe quick brown fox jumps over the lazy dog, repeatedly.\n",
            "one.md",
            &fleuron_markdown::Options::default(),
        )
        .0,
    );
    let styles = fleuron::style::defaults(&book, &registry);
    let laid_out = fleuron::layout::layout_book(&book, &styles, &registry, &Assets::none());

    let bytes = wire::encode(&laid_out).expect("a laid-out book encodes");
    let read = wire::decode(&bytes).expect("and reads back");
    assert_eq!(read.first, 0);
    assert_eq!(read.book_pages, laid_out.pages.len());
    assert_eq!(read.fonts, laid_out.fonts);
    assert_eq!(read.assets, laid_out.assets);
    assert_eq!(read.warnings, laid_out.warnings);
    assert_eq!(read.pages, laid_out.pages);
    assert_eq!(
        wire::encode(&as_output(read)).expect("and encodes again"),
        bytes
    );
}
