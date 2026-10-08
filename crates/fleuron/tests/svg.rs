//! Vector art through the whole engine: markdown in, pages out.
//!
//! Each test places an SVG, or puts one behind a box, and reads the
//! box it was given back off the display structure.

use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::layout_book;
use fleuron::pages::DrawItem;
use fleuron::style::{Source, Stylesheets};
use fleuron::{LayoutOutput, Warning};
use fleuron_markdown::Options;

/// A swelled rule, 2in by 0.25in.
const TAILPIECE: &[u8] = include_bytes!("../../../fixtures/images/tailpiece.svg");

/// A page whose content box is 300pt by 400pt.
const PAGE: &str = "@page { size: 348pt 448pt; margin: 24pt }\n";

const PROSE: &str = "I lay down on the grass, which was very short and soft, where I slept \
                     sounder than ever I remembered to have done in my life, and, as I \
                     reckoned, about nine hours; for when I awaked, it was just day-light.";

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// The host side: the fixture's rule, and roots that give a ratio, a
/// square and nothing.
struct Art;

impl ImageLoader for Art {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        let svg = |root: &str| format!("<svg xmlns=\"http://www.w3.org/2000/svg\" {root}/>");
        Some(match url {
            "tailpiece.svg" => TAILPIECE.to_vec(),
            "ratio.svg" => svg("viewBox=\"0 0 300 100\"").into_bytes(),
            "percent.svg" => {
                svg("width=\"100%\" height=\"100%\" viewBox=\"0 0 300 100\"").into_bytes()
            }
            "square.svg" => svg("width=\"48pt\" height=\"48pt\"").into_bytes(),
            "bare.svg" => svg("").into_bytes(),
            _ => return None,
        })
    }
}

fn lay_out(markdown: &str, css: &str) -> LayoutOutput {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "svg.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections);
    let styles = Stylesheets::parse(&[Source::author("svg.css", &format!("{PAGE}{css}"))])
        .compile(&book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings()
    );
    let assets = Assets::probe(&book, &styles, &Art);
    layout_book(&book, &styles, registry(), &assets)
}

/// Every placed image, as `[x, y, w, h]`, in paint order.
fn images(output: &LayoutOutput) -> Vec<[f32; 4]> {
    output
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            DrawItem::Image { x, y, w, h, .. } => Some([*x, *y, *w, *h]),
            _ => None,
        })
        .collect()
}

/// Every background of one page, as `(box, first tile)`.
fn backgrounds(output: &LayoutOutput, page: usize) -> Vec<([f32; 4], [f32; 4])> {
    output.pages[page]
        .items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Background {
                x,
                y,
                w,
                h,
                tile_x,
                tile_y,
                tile_w,
                tile_h,
                ..
            } => Some(([*x, *y, *w, *h], [*tile_x, *tile_y, *tile_w, *tile_h])),
            _ => None,
        })
        .collect()
}

fn about(warnings: &[Warning], url: &str) -> Vec<String> {
    warnings
        .iter()
        .filter(|warning| warning.message.contains(url))
        .map(|warning| warning.message.clone())
        .collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-2
}

/// Acceptance: an SVG a manuscript places sets at its intrinsic
/// size, and scales with `width` in a sheet.
#[test]
fn a_placed_svg_sets_at_its_own_size_and_scales_with_a_width() {
    let own = lay_out("![a rule](tailpiece.svg)\n", "");
    assert!(own.warnings.is_empty(), "{:?}", own.warnings);
    let [_, _, w, h] = images(&own)[0];
    assert_eq!((w, h), (144.0, 18.0), "2in by 0.25in");
    assert_eq!(own.assets[0].url, "tailpiece.svg");
    assert!(own.assets[0].intrinsic.sized);

    let scaled = lay_out("![a rule](tailpiece.svg)\n", "img { width: 72pt }");
    let [_, _, w, h] = images(&scaled)[0];
    assert_eq!((w, h), (72.0, 9.0));
}

/// Acceptance: an SVG with a `viewBox` and no size sets at the ratio
/// the `viewBox` gives. It fills the width it is placed in, and a
/// width in a sheet keeps the ratio.
#[test]
fn an_svg_with_a_view_box_and_no_size_sets_at_its_ratio() {
    for url in ["ratio.svg", "percent.svg"] {
        let markdown = format!("![a plate]({url})\n");
        let filled = lay_out(&markdown, "");
        assert!(filled.warnings.is_empty(), "{url}: {:?}", filled.warnings);
        let [_, _, w, h] = images(&filled)[0];
        assert!(close(w, 300.0) && close(h, 100.0), "{url}: {w} by {h}");
        assert!(!filled.assets[0].intrinsic.sized);

        let narrow = lay_out(&markdown, "img { width: 90pt }");
        let [_, _, w, h] = images(&narrow)[0];
        assert!(close(w, 90.0) && close(h, 30.0), "{url}: {w} by {h}");

        let short = lay_out(&markdown, "img { height: 20pt }");
        let [_, _, w, h] = images(&short)[0];
        assert!(close(w, 60.0) && close(h, 20.0), "{url}: {w} by {h}");
    }
}

/// Acceptance: `background-image: url("ornament.svg")` paints behind
/// a block and behind a page. An SVG with a size tiles at that size,
/// and one with a ratio alone is drawn as large as fits the box.
#[test]
fn an_svg_paints_behind_a_block_and_behind_a_page() {
    let output = lay_out(
        &format!("{PROSE}\n"),
        "@page { background-image: url(\"ratio.svg\"); background-repeat: no-repeat }\n\
         p { margin: 0; background-image: url(\"square.svg\") }",
    );
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let painted = backgrounds(&output, 0);
    assert_eq!(painted.len(), 2, "{painted:?}");

    // The page comes first: its box is the whole sheet, and a ratio
    // of three to one fits it across.
    let (sheet, tile) = painted[0];
    assert_eq!(sheet, [0.0, 0.0, 348.0, 448.0]);
    assert!(close(tile[2], 348.0) && close(tile[3], 116.0), "{tile:?}");

    let (block, tile) = painted[1];
    assert!(close(block[0], 24.0) && close(block[2], 300.0), "{block:?}");
    assert_eq!([tile[2], tile[3]], [48.0, 48.0]);
}

/// Acceptance: an SVG with neither a size nor a `viewBox` warns once
/// and is skipped, however often the book places it.
#[test]
fn an_svg_with_no_size_and_no_view_box_warns_once_and_is_skipped() {
    let output = lay_out(
        &format!("![one](bare.svg)\n\n{PROSE}\n\n![two](bare.svg)\n"),
        "p { background-image: url(bare.svg) }",
    );
    assert!(images(&output).is_empty());
    assert!(backgrounds(&output, 0).is_empty());
    assert!(output.assets.is_empty());
    assert_eq!(
        about(&output.warnings, "bare.svg"),
        ["Image bare.svg has no size in it. The image is skipped."],
    );
}

/// Part: `shape-outside: auto` on an SVG warns, and the text goes
/// around the box. The lines beside the image are the lines a sheet
/// that asks for the box sets.
#[test]
fn shape_outside_auto_on_an_svg_warns_and_wraps_the_box() {
    let markdown = format!("![a seal](square.svg)\n\n{PROSE}\n");
    let anchored = "img { position: absolute; top: 0; right: 0; wrap-flow: start; \
                    margin-left: 6pt }";
    let boxed = lay_out(&markdown, anchored);
    let traced = lay_out(
        &markdown,
        &format!("{anchored} img {{ shape-outside: auto }}"),
    );
    assert!(boxed.warnings.is_empty(), "{:?}", boxed.warnings);
    assert_eq!(
        about(&traced.warnings, "square.svg"),
        ["Missing alpha channel in square.svg. Text wraps around the image rectangle."],
    );
    assert_eq!(traced.pages, boxed.pages);

    // The first line stops short of the image.
    let [x, ..] = images(&traced)[0];
    let reach = traced.pages[0]
        .items
        .iter()
        .find_map(|item| match item {
            DrawItem::Text { x, width, .. } => Some(x + width),
            _ => None,
        })
        .expect("a line is set");
    assert!(
        reach <= x - 6.0 + 0.01,
        "the line reaches {reach}, the image starts at {x}"
    );
}
