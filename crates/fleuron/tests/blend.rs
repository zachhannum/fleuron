//! `mix-blend-mode`: what a sheet sets, and what the display structure
//! carries for it.
//!
//! One manuscript, a chapter with a quotation and an emphasis in it,
//! set under sheets that blend the quotation, the emphasis, or both.

use std::path::Path;

use fleuron::content::{Block, Book};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{BlendMode, Color, Source, StyleTree, Stylesheets};
use fleuron_markdown::Options;

/// A page large enough for the whole chapter, so every item is on
/// page one.
const PAGE_CSS: &str = "@page { size: 300pt 400pt; margin: 24pt }\n";

/// A heading, a quotation that holds an emphasis and an image, and a
/// paragraph that holds an emphasis.
const MANUSCRIPT: &str = "# The Quay\n\n\
                          > Quoted: the wind came off *the water* tonight.\n\
                          >\n\
                          > ![an ornament](images/fleuron.png)\n\n\
                          The tide *turned* before morning.\n";

/// The quotation's own tint.
const TINT: Color = Color::rgb(0xf4, 0xf1, 0xea);

/// The tint behind an emphasis.
const CHIP: Color = Color::rgb(0x85, 0x85, 0x85);

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

fn book() -> Book {
    read(MANUSCRIPT)
}

fn read(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "blend.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn styles(book: &Book, css: &str) -> StyleTree {
    let styles = Stylesheets::parse(&[Source::author("blend.css", &format!("{PAGE_CSS}{css}"))])
        .compile(book, registry());
    assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
    styles
}

fn page(css: &str) -> Page {
    page_of(&book(), css)
}

fn page_of(book: &Book, css: &str) -> Page {
    let styles = styles(book, css);
    let assets = Assets::probe(book, &styles, &Fixtures);
    let mut pages = layout_book(book, &styles, registry(), &assets).pages;
    assert_eq!(pages.len(), 1, "the chapter fits one page");
    pages.remove(0)
}

/// The mode of every run whose text holds `words`.
fn runs(page: &Page, words: &str) -> Vec<BlendMode> {
    let found: Vec<BlendMode> = page
        .items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text { text, blend, .. } if text.contains(words) => Some(*blend),
            _ => None,
        })
        .collect();
    assert!(!found.is_empty(), "no run holds {words:?}");
    found
}

/// The mode of every rect filled with `fill`.
fn rects(page: &Page, fill: Color) -> Vec<BlendMode> {
    let found: Vec<BlendMode> = page
        .items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rect { color, blend, .. } if *color == fill => Some(*blend),
            _ => None,
        })
        .collect();
    assert!(!found.is_empty(), "nothing is filled in {}", fill.to_hex());
    found
}

/// The mode of every image.
fn images(page: &Page) -> Vec<BlendMode> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Image { blend, .. } => Some(*blend),
            _ => None,
        })
        .collect()
}

/// Acceptance: the parser accepts `normal` and the fifteen blend
/// modes, in any case, and warns on any other value.
#[test]
fn the_parser_reads_sixteen_modes_and_warns_on_any_other_value() {
    let book = book();
    let Block::Blockquote { id, .. } = &book.sections[0].blocks[1] else {
        unreachable!("the manuscript's second block is the quotation")
    };
    assert_eq!(BlendMode::ALL.len(), 16);
    for mode in BlendMode::ALL {
        for keyword in [mode.keyword().to_string(), mode.keyword().to_uppercase()] {
            let styles = styles(
                &book,
                &format!("blockquote {{ mix-blend-mode: {keyword} }}"),
            );
            assert_eq!(styles.style(*id).mix_blend_mode, mode, "{keyword}");
        }
    }
    for value in ["plus-lighter", "multiply screen", "1", "none"] {
        let sheet = Stylesheets::parse(&[Source::author(
            "blend.css",
            &format!("p {{\n  mix-blend-mode: {value};\n}}"),
        )]);
        let [warning] = sheet.warnings() else {
            panic!("{value} raised {:?}", sheet.warnings())
        };
        assert!(
            warning
                .origin
                .as_deref()
                .is_some_and(|at| at.contains("2:3")),
            "{warning:?}"
        );
    }
}

/// Acceptance: the text, the rules, the image, the border and the
/// background of a block carry the block's mode, and the blocks
/// outside it carry `normal`.
#[test]
fn everything_a_block_draws_carries_its_mode() {
    let page = page(
        "blockquote { mix-blend-mode: multiply; background-color: #f4f1ea; \
                      border: 2pt solid #123456; text-decoration-line: underline; \
                      text-decoration-color: #654321 }",
    );
    assert!(
        runs(&page, "Quoted")
            .iter()
            .all(|mode| *mode == BlendMode::Multiply)
    );
    assert_eq!(rects(&page, TINT), [BlendMode::Multiply]);
    assert_eq!(
        rects(&page, Color::rgb(0x12, 0x34, 0x56)),
        [BlendMode::Multiply; 4]
    );
    assert!(
        rects(&page, Color::rgb(0x65, 0x43, 0x21))
            .iter()
            .all(|mode| *mode == BlendMode::Multiply)
    );
    assert_eq!(images(&page), [BlendMode::Multiply]);
    assert_eq!(runs(&page, "The Quay"), [BlendMode::Normal]);
    assert!(
        runs(&page, "tide")
            .iter()
            .all(|mode| *mode == BlendMode::Normal)
    );
}

/// Acceptance: the text and the box of an inline element carry the
/// element's mode, and the text of the paragraph around it carries
/// `normal`.
#[test]
fn an_inline_element_carries_its_mode_and_the_text_around_it_does_not() {
    let page = page(
        "p { background-color: #f4f1ea } \
         em { mix-blend-mode: difference; background-color: #858585; padding: 0 2pt }",
    );
    assert_eq!(runs(&page, "turned"), [BlendMode::Difference]);
    assert_eq!(runs(&page, "the water"), [BlendMode::Difference]);
    assert_eq!(rects(&page, CHIP), [BlendMode::Difference; 2]);
    for words in ["The tide", "before morning", "Quoted"] {
        assert_eq!(runs(&page, words), [BlendMode::Normal], "{words}");
    }
    assert!(
        rects(&page, TINT)
            .iter()
            .all(|mode| *mode == BlendMode::Normal)
    );
}

/// Acceptance: an element with no mode of its own takes the mode of
/// the nearest element around it that has one. The property itself
/// does not inherit.
#[test]
fn an_element_takes_the_mode_of_the_nearest_element_around_it() {
    let book = book();
    let Block::Blockquote { blocks, .. } = &book.sections[0].blocks[1] else {
        unreachable!("the manuscript's second block is the quotation")
    };
    let Block::Paragraph { id: quoted, .. } = &blocks[0] else {
        unreachable!("the quotation opens with a paragraph")
    };
    let css = "blockquote { mix-blend-mode: multiply } em { background-color: #858585 }";
    let quoted = styles(&book, css).style(*quoted).clone();
    assert_eq!(quoted.mix_blend_mode, BlendMode::Normal);
    assert_eq!(quoted.blend(), BlendMode::Multiply);

    let around = page(css);
    assert_eq!(runs(&around, "the water"), [BlendMode::Multiply]);
    assert_eq!(
        rects(&around, CHIP),
        [BlendMode::Multiply, BlendMode::Normal],
        "the chip in the quotation, then the one outside it",
    );

    let nearest = page(&format!("{css} em {{ mix-blend-mode: screen }}"));
    assert_eq!(runs(&nearest, "the water"), [BlendMode::Screen]);
    assert_eq!(rects(&nearest, CHIP), [BlendMode::Screen; 2]);
    assert_eq!(runs(&nearest, "Quoted"), [BlendMode::Multiply]);
}

/// Part of the same: a table and an image the sheet puts against the
/// page are built apart from the blocks around them, and they carry
/// their modes as well.
#[test]
fn a_table_and_an_image_against_the_page_carry_their_modes() {
    let book = read(
        "# The Quay\n\n\
         | Tide | Hour |\n|---|---|\n| High | Six |\n\n\
         ![an ornament](images/fleuron.png)\n",
    );
    let page = page_of(
        &book,
        "table { mix-blend-mode: multiply } \
         tr { background-color: #f4f1ea } \
         td { border: 1pt solid #858585 } \
         img { position: absolute; top: 0; left: 0; mix-blend-mode: screen }",
    );
    for words in ["Tide", "High", "Six"] {
        assert_eq!(runs(&page, words), [BlendMode::Multiply], "{words}");
    }
    assert!(
        rects(&page, TINT)
            .iter()
            .all(|mode| *mode == BlendMode::Multiply)
    );
    assert!(
        rects(&page, CHIP)
            .iter()
            .all(|mode| *mode == BlendMode::Multiply)
    );
    assert_eq!(images(&page), [BlendMode::Screen]);
    assert_eq!(runs(&page, "The Quay"), [BlendMode::Normal]);
}
