//! Inline boxes: what a background, a padding and a border on a run
//! paint, and what they do to the line around them.
//!
//! One fixture, a paragraph with a tagged run in it, set under a
//! sheet that tints the tag.

use fleuron::content::{Attributes, Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::pages::{Corners, DrawItem, Page};
use fleuron::style::{Color, Source, StyleTree, Stylesheets};

/// A measure wide enough for the tagged run and the words around it.
const CHIP_CSS: &str = r#"
@page { size: 300pt 200pt; margin: 20pt }

p { margin: 0; text-indent: 0 }

code {
  background-color: #858585;
  color: white;
  padding: 2pt 4pt;
}
"#;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

fn code(value: &str) -> Inline {
    Inline::Code {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

fn paragraph(inlines: Vec<Inline>) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines,
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// A book of one paragraph.
fn book_of(inlines: Vec<Inline>) -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            attributes: Default::default(),
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![paragraph(inlines)],
            position: None,
            span: None,
        }],
    };
    book.assign_node_ids();
    book
}

/// The fixture: a line with one tagged run in the middle of it.
fn tagged() -> Book {
    book_of(vec![
        text("roll for "),
        code("2d6"),
        text(" and add the modifier"),
    ])
}

/// The same with a tag long enough to break over two lines.
fn broken() -> Book {
    book_of(vec![
        text("roll "),
        code("2d6 plus the modifier you wrote down"),
    ])
}

/// A measure the tag of `broken` does not fit on one line of.
fn broken_css() -> String {
    format!("{CHIP_CSS}\n@page {{ size: 160pt 200pt; margin: 20pt }}")
}

fn pages(book: &Book, css: &str) -> Vec<Page> {
    let styles: StyleTree =
        Stylesheets::parse(&[Source::author("chip.css", css)]).compile(book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings(),
    );
    Paginator::new(registry(), &styles).paginate(book)
}

/// One page's filled rects, in paint order.
fn rects(page: &Page) -> Vec<(f32, f32, f32, f32, Color)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rect {
                x, y, w, h, color, ..
            } => Some((*x, *y, *w, *h, *color)),
            _ => None,
        })
        .collect()
}

/// One page's rounded boxes, in paint order.
fn rounded(page: &Page) -> Vec<(f32, f32, f32, f32, Corners)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rounded {
                x, y, w, h, radii, ..
            } => Some((*x, *y, *w, *h, *radii)),
            _ => None,
        })
        .collect()
}

/// Every run of one page: its text, its leading edge, its baseline,
/// and where its glyphs end.
fn runs(page: &Page) -> Vec<(String, f32, f32, f32)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                x, y, text, glyphs, ..
            } => {
                let end = glyphs.last().map(|glyph| glyph.x).unwrap_or(*x);
                Some((text.clone(), *x, *y, end))
            }
            _ => None,
        })
        .collect()
}

/// The runs a tagged book set, by the text they hold.
fn run_named<'a>(runs: &'a [(String, f32, f32, f32)], text: &str) -> &'a (String, f32, f32, f32) {
    runs.iter()
        .find(|run| run.0.trim() == text)
        .unwrap_or_else(|| panic!("no run holding {text:?}: {runs:?}"))
}

fn close(got: f32, want: f32) -> bool {
    (got - want).abs() < 0.01
}

/// Acceptance: a tint on a code span covers the run and no more of
/// the line. The rest of the line is outside it on both sides.
#[test]
fn a_tinted_run_is_tinted_and_the_line_is_not() {
    let pages = pages(&tagged(), CHIP_CSS);
    let painted = rects(&pages[0]);
    assert_eq!(painted.len(), 1, "one tint and no more: {painted:?}");
    let (x, _, w, _, color) = painted[0];
    assert_eq!(color, Color::rgb(0x85, 0x85, 0x85));

    let runs = runs(&pages[0]);
    let tagged = run_named(&runs, "2d6");
    let before = run_named(&runs, "roll for");
    let after = run_named(&runs, "and add the modifier");
    assert!(
        close(x, tagged.1 - 4.0),
        "the tint does not open a padding before the run: {x} against {}",
        tagged.1,
    );
    assert!(
        x + w > tagged.3,
        "the tint stops short of the run it sits behind: {painted:?} {runs:?}",
    );
    assert!(
        close(x + w, after.1),
        "the tint reaches past its own padding and into the prose: {painted:?} {runs:?}",
    );
    assert!(before.3 < x, "the tint reaches the words before the tag");
}

/// Acceptance: the padding is width. The run after the tag is moved
/// along by the padding on both sides of it, and the measure the
/// paragraph needs to stay on one line grows by the same amount.
#[test]
fn padding_is_width_the_line_is_broken_against() {
    let book = tagged();
    let padded = runs(&pages(&book, CHIP_CSS)[0]);
    let plain = runs(&pages(&book, &format!("{CHIP_CSS}\ncode {{ padding: 0 }}"))[0]);
    let moved =
        run_named(&padded, "and add the modifier").1 - run_named(&plain, "and add the modifier").1;
    assert!(
        close(moved, 8.0),
        "the prose after the tag moved by {moved}pt, not by the padding",
    );

    // The narrowest measure the paragraph still sets in one line.
    // The padding is charged to the breaker, so the padded paragraph
    // asks for the padding more than the plain one does.
    let step = 0.25;
    let narrowest = |css: &str| -> f32 {
        (400..1000)
            .map(|steps| steps as f32 * step)
            .find(|measure| {
                let page = format!("@page {{ size: {}pt 200pt; margin: 20pt }}", measure + 40.0);
                let pages = pages(&book, &format!("{css}\n{page}"));
                let mut baselines: Vec<f32> =
                    runs(&pages[0]).into_iter().map(|run| run.2).collect();
                baselines.dedup();
                baselines.len() == 1
            })
            .expect("the paragraph sets in one line at some measure")
    };
    let grew = narrowest(CHIP_CSS) - narrowest(&format!("{CHIP_CSS}\ncode {{ padding: 0 }}"));
    assert!(
        (grew - 8.0).abs() <= step,
        "the measure the padded paragraph needs grew by {grew}pt, not by the padding",
    );
}

/// Acceptance: an inline broken over two lines paints on both, and
/// `slice` leaves the edges the break cut open.
#[test]
fn a_broken_inline_paints_on_both_lines() {
    let pages = pages(&broken(), &broken_css());
    let painted = rects(&pages[0]);
    assert_eq!(
        painted.len(),
        2,
        "the tag did not break in two: {painted:?}"
    );
    assert!(
        painted[0].1 < painted[1].1,
        "the two fragments share a baseline: {painted:?}",
    );
    let runs = runs(&pages[0]);
    let opening = run_named(&runs, "2d6 plus the modifier");
    let closing = runs.last().expect("the tag continues");
    assert!(
        close(painted[0].0, opening.1 - 4.0),
        "the opening fragment lost the padding before the tag: {painted:?} {runs:?}",
    );
    assert!(
        close(painted[1].0, closing.1),
        "the continuing fragment closed an edge the break cut: {painted:?} {runs:?}",
    );
    assert!(
        painted[1].0 + painted[1].2 > closing.3 + 4.0,
        "the last fragment lost the padding after the tag: {painted:?} {runs:?}",
    );
}

/// Acceptance: `box-decoration-break: clone` closes both edges of
/// both fragments, so each of them is the padding wider.
#[test]
fn a_cloned_inline_closes_both_edges_of_both_fragments() {
    let sliced = rects(&pages(&broken(), &broken_css())[0]);
    let cloned = rects(
        &pages(
            &broken(),
            &format!("{}\ncode {{ box-decoration-break: clone }}", broken_css()),
        )[0],
    );
    assert_eq!(sliced.len(), 2);
    assert_eq!(
        cloned.len(),
        2,
        "the cloned tag broke elsewhere: {cloned:?}"
    );
    for (index, (sliced, cloned)) in sliced.iter().zip(&cloned).enumerate() {
        assert!(
            close(cloned.0, sliced.0),
            "fragment {index} opens elsewhere when it is cloned",
        );
        assert!(
            close(cloned.2, sliced.2 + 4.0),
            "fragment {index} did not close the edge the break cut: \
             {}pt against {}pt",
            cloned.2,
            sliced.2,
        );
    }
}

/// Acceptance: a rounded chip is a rounded box in the display
/// structure, which is what both painters draw its corners from.
#[test]
fn a_rounded_chip_is_a_rounded_box() {
    let css = format!("{CHIP_CSS}\ncode {{ border-radius: 3pt }}");
    let pages = pages(&tagged(), &css);
    let painted = rounded(&pages[0]);
    assert_eq!(painted.len(), 1, "the chip is not one rounded box");
    let (_, _, _, _, radii) = painted[0];
    assert_eq!(radii.top_left.x, 3.0);
    assert_eq!(radii.bottom_right.y, 3.0);
    assert!(
        rects(&pages[0]).is_empty(),
        "the chip was painted square too"
    );
}

/// Acceptance: padding above and below a run leaves the lines around
/// it where they were.
#[test]
fn vertical_padding_does_not_move_the_lines() {
    let book = book_of(vec![
        text("roll for "),
        code("2d6"),
        text(" and add the modifier, then read the result off the table below it"),
    ]);
    let css = format!("{CHIP_CSS}\n@page {{ size: 160pt 200pt; margin: 20pt }}");
    let tall = format!("{css}\ncode {{ padding: 6pt 4pt }}");
    let baselines = |css: &str| -> Vec<f32> {
        let mut seen: Vec<f32> = runs(&pages(&book, css)[0])
            .into_iter()
            .map(|run| run.2)
            .collect();
        seen.dedup();
        seen
    };
    assert!(baselines(&css).len() > 1, "the paragraph did not break");
    assert_eq!(
        baselines(&css),
        baselines(&tall),
        "the padding above and below the tag moved the lines",
    );
}

/// Alignment moves the line, and the chip moves with it. The tint
/// opens its padding before the tagged run and closes it after,
/// whichever way the line is set.
#[test]
fn a_chip_moves_with_the_line_that_alignment_moves() {
    let mut left_edges = Vec::new();
    for align in ["left", "center", "right"] {
        let css = format!("{CHIP_CSS}\np {{ text-align: {align} }}");
        let pages = pages(&tagged(), &css);
        let painted = rects(&pages[0]);
        assert_eq!(painted.len(), 1, "{align}: one tint: {painted:?}");
        let (x, _, w, _, _) = painted[0];
        let runs = runs(&pages[0]);
        let tag = run_named(&runs, "2d6");
        let after = run_named(&runs, "and add the modifier");
        assert!(
            close(x, tag.1 - 4.0),
            "{align}: the tint opens at {x} and the run at {}",
            tag.1,
        );
        assert!(
            close(x + w, after.1),
            "{align}: the tint closes at {} and the prose opens at {}",
            x + w,
            after.1,
        );
        left_edges.push(x);
    }
    assert!(
        left_edges[0] < left_edges[1] && left_edges[1] < left_edges[2],
        "alignment did not move the line: {left_edges:?}",
    );
}

/// The tint is painted before the run it sits behind: `DrawItem`
/// order is paint order.
#[test]
fn a_chip_paints_before_the_run_it_tints() {
    let pages = pages(&tagged(), CHIP_CSS);
    let tint = pages[0]
        .items
        .iter()
        .position(|item| matches!(item, DrawItem::Rect { .. }))
        .expect("the chip was painted");
    let tagged = pages[0]
        .items
        .iter()
        .position(|item| matches!(item, DrawItem::Text { text, .. } if text == "2d6"))
        .expect("the tag was set");
    assert!(tint < tagged, "the run was painted under its own tint");
}

/// A chip travels with the block the sheet takes out of the flow and
/// puts against the page. The box of an inline element is painted
/// with the fragments of its own paragraph, so it moves with them.
#[test]
fn a_chip_travels_with_the_block_the_sheet_anchors() {
    let quoted = Block::Blockquote {
        id: NodeId::UNASSIGNED,
        blocks: vec![paragraph(vec![
            text("roll for "),
            code("2d6"),
            text(" and add the modifier"),
        ])],
        attributes: Attributes::default(),
        position: None,
        span: None,
    };
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            attributes: Default::default(),
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![paragraph(vec![text("a line of prose")]), quoted],
            position: None,
            span: None,
        }],
    };
    book.assign_node_ids();
    let css = format!(
        "{CHIP_CSS}\nblockquote {{ position: absolute; top: 40pt; left: 10pt; margin: 0 }}"
    );
    let pages = pages(&book, &css);
    let painted = rects(&pages[0]);
    assert_eq!(painted.len(), 1, "one tint and no more: {painted:?}");
    let (x, y, w, h, _) = painted[0];

    // The quotation sits 10pt into the content box, so the chip is
    // past that, and the line above it stays where it was.
    let runs = runs(&pages[0]);
    let tag = run_named(&runs, "2d6");
    let flowed = run_named(&runs, "a line of prose");
    assert!(x > 30.0, "the chip did not travel with the block: {x}");
    assert!(
        close(x, tag.1 - 4.0) && close(x + w, run_named(&runs, "and add the modifier").1),
        "the chip lost the run it sits behind: {painted:?} {runs:?}",
    );
    assert!(
        tag.2 > y && tag.2 < y + h,
        "the run is not inside the chip: {painted:?} {tag:?}",
    );
    assert!(flowed.2 < y, "the anchored block did not leave the flow");
}
