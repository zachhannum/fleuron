//! Image sizes through the whole engine: markdown in, pages out.
//!
//! Each test reads a manuscript with plates in it, lays it out under
//! the built-in sheet and a few rules of its own, and reads the size
//! of every plate back off the display structure.

use std::path::Path;

use fleuron::LayoutOutput;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::layout_book;
use fleuron::pages::DrawItem;
use fleuron::style::{Source, Stylesheets};
use fleuron_markdown::Options;

/// The fixture: a map, an ornament inside a quotation, and an
/// ornament outside one.
const FIXTURE: &str = include_str!("../../../fixtures/plates.md");

/// Three ornaments, and nothing else in their section.
const ORNAMENTS: &str = "![one](images/fleuron.png)\n\n\
                         ![two](images/fleuron.png)\n\n\
                         ![three](images/fleuron.png)\n";

/// The ornament is 128px at 300dpi.
const ORNAMENT: f32 = 128.0 / 300.0 * 72.0;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// The fixture book's images, by their path under `fixtures/`.
struct Fixtures;

impl ImageLoader for Fixtures {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures")
                .join(url),
        )
        .ok()
    }
}

fn lay_out(markdown: &str, css: &str) -> LayoutOutput {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "plates.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    let book = fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections);
    let styles =
        Stylesheets::parse(&[Source::author("plates.css", css)]).compile(&book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings()
    );
    let assets = Assets::probe(&book, &styles, &Fixtures);
    layout_book(&book, &styles, registry(), &assets)
}

/// Every plate of every page, as `(width, height)`, in paint order.
fn plates(output: &LayoutOutput) -> Vec<(f32, f32)> {
    output
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            DrawItem::Image { w, h, .. } => Some((*w, *h)),
            _ => None,
        })
        .collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-2
}

fn scaled_to_fit(output: &LayoutOutput, too: &str) -> usize {
    output
        .warnings
        .iter()
        .filter(|warning| warning.message.contains(too) && warning.message.contains("scaled"))
        .count()
}

/// Acceptance: `img { width: 200pt }` sets every plate 200pt wide,
/// and its height follows the intrinsic ratio.
#[test]
fn a_width_sets_every_plate_that_wide_and_the_height_follows() {
    let own = plates(&lay_out(FIXTURE, ""));
    let sized = plates(&lay_out(FIXTURE, "img { width: 200pt }"));
    assert_eq!(own.len(), 3);
    assert_eq!(sized.len(), own.len());
    for ((width, height), (own_width, own_height)) in sized.into_iter().zip(own) {
        assert_eq!(width, 200.0);
        assert!(
            close(height / width, own_height / own_width),
            "{width} by {height} is not the ratio of {own_width} by {own_height}",
        );
    }
}

/// Acceptance: a percentage width resolves against the width the image
/// is in, a quotation's included.
#[test]
fn a_percentage_width_measures_the_width_the_image_is_in() {
    let skip_map = "img.map { width: auto }";
    let full = plates(&lay_out(
        FIXTURE,
        &format!("img {{ width: 100% }} {skip_map}"),
    ));
    let half = plates(&lay_out(
        FIXTURE,
        &format!("img {{ width: 50% }} {skip_map}"),
    ));
    let (quoted, outside) = (1, 2);
    assert!(
        full[quoted].0 < full[outside].0,
        "the quotation is no narrower: {full:?}",
    );
    for index in [quoted, outside] {
        assert!(
            close(half[index].0, full[index].0 / 2.0),
            "{half:?} is not half of {full:?}",
        );
        assert!(close(half[index].1, half[index].0), "{half:?}");
    }
}

/// Acceptance: `img:nth-child(2) { width: 120pt }` sizes that plate and
/// no other.
#[test]
fn a_selector_sizes_the_plate_it_selects_and_no_other() {
    let widths: Vec<f32> = plates(&lay_out(ORNAMENTS, "img:nth-child(2) { width: 120pt }"))
        .into_iter()
        .map(|(width, _)| width)
        .collect();
    assert_eq!(widths.len(), 3);
    assert!(close(widths[0], ORNAMENT), "{widths:?}");
    assert_eq!(widths[1], 120.0);
    assert!(close(widths[2], ORNAMENT), "{widths:?}");
}

/// Acceptance: a size larger than the content box is scaled to fit it,
/// and warns.
#[test]
fn a_size_larger_than_the_content_box_scales_to_fit_and_warns() {
    let fitted = lay_out(ORNAMENTS, "img { width: 100% }");
    assert_eq!(scaled_to_fit(&fitted, "wider"), 0);

    // The three plates share a url, and a warning is said once.
    let wide = lay_out(ORNAMENTS, "img { width: 400pt }");
    assert_eq!(plates(&wide), plates(&fitted));
    assert_eq!(scaled_to_fit(&wide, "wider"), 1);

    // Both sides given, so the height alone is too much for the page.
    let tall = lay_out(ORNAMENTS, "img { width: 200pt; height: 2000pt }");
    let page = lay_out(ORNAMENTS, "img { width: 200pt; height: 100% }");
    assert_eq!(scaled_to_fit(&tall, "taller"), 1);
    assert_eq!(scaled_to_fit(&page, "taller"), 0);
    let heights = |output: &LayoutOutput| -> Vec<f32> {
        plates(output)
            .into_iter()
            .map(|(_, height)| height)
            .collect()
    };
    assert_eq!(heights(&tall), heights(&page));
    assert_eq!(
        tall.pages.len(),
        page.pages.len(),
        "a scaled image left the rest of its height below it",
    );
}

/// Every text run and image of every page, one line each, in paint
/// order.
fn described(output: &LayoutOutput) -> Vec<Vec<String>> {
    output
        .pages
        .iter()
        .map(|page| {
            page.items
                .iter()
                .filter_map(|item| match item {
                    DrawItem::Text {
                        x, y, size, text, ..
                    } => Some(format!("text {x:.2} {y:.2} {size} {text:?}")),
                    DrawItem::Image { x, y, w, h, .. } => {
                        Some(format!("image {x:.2} {y:.2} {w:.2} {h:.2}"))
                    }
                    DrawItem::Rect { .. } | DrawItem::Background { .. } => None,
                })
                .collect()
        })
        .collect()
}

/// Acceptance: the display structure of the fixture with sized plates,
/// under snapshot.
#[test]
fn the_sized_plates_lay_out_to_the_display_list_they_are_checked_in_as() {
    let css = "img { width: 200pt } \
               img.map { max-height: 3in } \
               blockquote img { width: 25% }";
    insta::assert_json_snapshot!("plates_fixture", described(&lay_out(FIXTURE, css)));
}
