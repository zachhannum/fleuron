//! Property tests for the wire: what the engine writes, a host reads
//! back unchanged.
//!
//! The display structure crosses the boundary once per keystroke and is
//! decoded by someone else's code. So the property that matters is
//! not that the bytes parse but that nothing is lost on the way
//! through: encode, decode, encode again, and the second buffer is
//! the first one.

use fleuron::images::{Asset, Assets, Intrinsic};
use fleuron::pages::{DrawItem, Glyph, Page, Side};
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

fn text_item() -> impl Strategy<Value = DrawItem> {
    (
        coordinate(),
        coordinate(),
        any::<u16>(),
        1.0f32..200.0,
        ".{0,40}",
        proptest::collection::vec(glyph(), 0..12),
    )
        .prop_map(|(x, y, font_id, size, text, glyphs)| DrawItem::Text {
            x,
            y,
            font_id,
            size,
            text,
            glyphs,
        })
}

fn item() -> impl Strategy<Value = DrawItem> {
    prop_oneof![
        text_item(),
        (coordinate(), coordinate(), coordinate(), coordinate())
            .prop_map(|(x, y, w, h)| DrawItem::Rect { x, y, w, h }),
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

fn page() -> impl Strategy<Value = Page> {
    (
        1u32..2000,
        1.0f32..2000.0,
        1.0f32..2000.0,
        proptest::collection::vec(item(), 0..8),
    )
        .prop_map(|(number, width, height, items)| Page {
            number,
            side: Side::of_number(number),
            width,
            height,
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
        prop_assert_eq!(&read, &output);
        prop_assert_eq!(wire::encode(&read).expect("and encodes again"), bytes);
    }

    /// The version leads every buffer, so a host can refuse one it
    /// does not know without decoding a page of it.
    #[test]
    fn the_version_leads_every_buffer(output in output()) {
        let bytes = wire::encode(&output).expect("a display structure encodes");
        prop_assert_eq!(wire::version(&bytes).expect("the version reads"), wire::VERSION);
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
    assert_eq!(read, laid_out);
    assert_eq!(wire::encode(&read).expect("and encodes again"), bytes);
}
