//! Lists through the whole engine: markdown in, pages out.
//!
//! Each test reads a list the way the frontend reads one, lays it out
//! under the built-in sheet and a few rules of its own, and reads the
//! markers and the items back off the display structure.

use fleuron::LayoutOutput;
use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::{Assets, ImageLoader};
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::{PageQuery, Situation, Source, StyleTree, Stylesheets};
use fleuron_markdown::Options;

/// The fixture: the articles of Gulliver's liberty, with a tight
/// list, a loose list, a nested list, and a list that counts from 7.
const FIXTURE: &str = include_str!("../../../fixtures/lists.md");

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A manuscript read the way the frontend reads one.
fn read(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "lists.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn styled(book: &Book, css: &str) -> StyleTree {
    let styles = Stylesheets::parse(&[Source::author("lists.css", css)]).compile(book, registry());
    assert!(
        styles.warnings().is_empty(),
        "the sheet is in the subset: {:?}",
        styles.warnings()
    );
    styles
}

fn lay_out(markdown: &str, css: &str) -> LayoutOutput {
    let book = read(markdown);
    let styles = styled(&book, css);
    layout_book(&book, &styles, registry(), &Assets::none())
}

/// The indent the built-in sheet gives one level of list, and the size
/// it sets body text in.
fn indent_and_size() -> (f32, f32) {
    let book = read("- one\n");
    let styles = fleuron::style::defaults(&book, registry());
    let node = styles
        .nodes()
        .iter()
        .find(|node| node.element == "ul")
        .expect("the book has a list");
    (
        styles.styles()[node.style as usize].padding.left,
        styles.root().font_size,
    )
}

/// One run of text as a page paints it.
#[derive(Debug)]
struct Run {
    x: f32,
    right: f32,
    y: f32,
    size: f32,
    text: String,
    named: bool,
}

/// The runs of one page, the folio left out.
fn runs(page: &Page) -> Vec<Run> {
    let folio = 9.0;
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                x,
                y,
                text,
                glyphs,
                font_id,
                size,
                origin,
                ..
            } if *size != folio => {
                let upem = registry().metrics(*font_id)?.units_per_em as f32;
                let right = glyphs
                    .iter()
                    .rev()
                    .find(|glyph| {
                        !text[glyph.range.start as usize..glyph.range.end as usize]
                            .trim()
                            .is_empty()
                    })
                    .map(|glyph| {
                        let advance = registry().advance_width(*font_id, glyph.id).unwrap_or(0);
                        glyph.x + advance as f32 / upem * size
                    })
                    .unwrap_or(*x);
                Some(Run {
                    x: *x,
                    right,
                    y: *y,
                    size: *size,
                    text: text.clone(),
                    named: origin.is_some(),
                })
            }
            _ => None,
        })
        .collect()
}

/// The first run anywhere in the book that starts with `text`, and the
/// index of the page it is on.
fn run(output: &LayoutOutput, text: &str) -> (usize, Run) {
    output
        .pages
        .iter()
        .enumerate()
        .flat_map(|(index, page)| runs(page).into_iter().map(move |run| (index, run)))
        .find(|(_, run)| run.text.starts_with(text))
        .unwrap_or_else(|| panic!("nothing says {text:?}"))
}

/// Every marker the book paints, in paint order: the runs no content
/// node was shaped from.
fn markers(output: &LayoutOutput) -> Vec<Run> {
    output
        .pages
        .iter()
        .flat_map(runs)
        .filter(|run| !run.named)
        .collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.05
}

/// Acceptance: a list in the fixture manuscript sets with its markers.
/// The frontend reads the fixture without a warning, and every item
/// has a marker to the left of its first line, on its baseline.
#[test]
fn a_list_sets_with_its_markers_and_the_frontend_warns_about_nothing() {
    let output = lay_out(FIXTURE, "");
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    for (marker, item) in [
        ("\u{2022}", "my scimitar"),
        ("1.", "The man-mountain shall not"),
        ("2.", "He shall not presume"),
        ("\u{2022}", "In the right coat-pocket"),
        ("\u{25E6}", "one great piece"),
        ("7.", "That the said man-mountain shall, at his"),
    ] {
        let (page, item) = run(&output, item);
        let placed = runs(&output.pages[page]);
        let found = placed
            .iter()
            .filter(|run| !run.named && run.text.trim() == marker)
            .find(|run| close(run.y, item.y))
            .unwrap_or_else(|| panic!("no {marker} on the baseline of {:?}", item.text));
        assert!(
            found.right < item.x,
            "the marker {marker} runs into {:?}",
            item.text
        );
        assert!(found.size == item.size);
    }
}

/// Acceptance: an ordered list starting at 7 numbers its items 7, 8
/// and 9, and each number ends at the same distance from its item.
#[test]
fn an_ordered_list_starting_at_seven_numbers_seven_eight_nine() {
    let output = lay_out("7. seven\n8. eight\n9. nine\n", "");
    let numbers: Vec<String> = markers(&output)
        .iter()
        .map(|run| run.text.trim().to_string())
        .collect();
    assert_eq!(numbers, ["7.", "8.", "9."]);
    for (number, item) in markers(&output).iter().zip(["seven", "eight", "nine"]) {
        let (_, item) = run(&output, item);
        assert!(close(number.y, item.y));
        assert!(number.right < item.x);
    }
}

/// `list-style-type` chooses the marker, and `none` leaves the item
/// without one.
#[test]
fn list_style_type_chooses_the_marker() {
    let marked = |css: &str| -> Vec<String> {
        markers(&lay_out("- one\n- two\n", css))
            .iter()
            .map(|run| run.text.trim().to_string())
            .collect()
    };
    assert_eq!(marked(""), ["\u{2022}", "\u{2022}"]);
    assert_eq!(
        marked("ul { list-style-type: square }"),
        ["\u{25A0}", "\u{25A0}"]
    );
    assert_eq!(marked("ul { list-style-type: lower-roman }"), ["i.", "ii."]);
    assert!(marked("ul { list-style-type: none }").is_empty());
}

/// The leading edge of the content box on a page of `side`. The
/// built-in sheet mirrors the margins, so the two sides differ.
fn left_of(side: Side) -> f32 {
    let book = read("Prose.\n");
    styled(&book, "")
        .page(PageQuery {
            name: Some("chapter"),
            situation: Situation::Body(side),
        })
        .geometry
        .content_origin()
        .0
}

/// Acceptance: a nested list indents twice, and keeps both indents on
/// every page it runs over.
#[test]
fn a_nested_list_indents_twice_and_keeps_both_indents_across_a_page() {
    let inner = "inner ".repeat(900);
    let output = lay_out(&format!("- outer\n  - {inner}\n"), "");
    let (indent, _) = indent_and_size();
    assert!(indent > 0.0, "the sheet indents nothing");

    let (_, outer) = run(&output, "outer");
    assert!(close(outer.x, left_of(Side::Recto) + indent));
    let mut spanned = 0;
    for page in &output.pages {
        let left = left_of(page.side);
        let lines: Vec<Run> = runs(page)
            .into_iter()
            .filter(|run| run.named && run.text.starts_with("inner"))
            .collect();
        spanned += !lines.is_empty() as usize;
        for line in lines {
            assert!(
                close(line.x, left + 2.0 * indent),
                "page {}: the nested list is {} in, not {}",
                page.number,
                line.x - left,
                2.0 * indent,
            );
        }
    }
    assert!(spanned >= 2, "the nested list fitted on {spanned} page(s)");
}

/// A book of `lines` one-line paragraphs, then a list of `items` short
/// items.
fn filler_then_list(lines: usize, items: usize) -> String {
    let mut markdown = String::new();
    for line in 0..lines {
        markdown.push_str(&format!("Filler line {line}.\n\n"));
    }
    for item in 1..=items {
        markdown.push_str(&format!("- item {item:02}\n"));
    }
    markdown
}

/// Acceptance: a list broken across a page does not leave its first
/// item alone at the foot of a page or its last item alone at the top
/// of one. Every place the page can fall over the list is tried.
#[test]
fn a_list_broken_across_a_page_leaves_neither_its_first_nor_its_last_item_alone() {
    let mut split = 0;
    for lines in 20..48 {
        let output = lay_out(&filler_then_list(lines, 6), "");
        let page = |item: usize| run(&output, &format!("item {item:02}")).0;
        assert_eq!(
            page(1),
            page(2),
            "item 01 was left alone after {lines} lines"
        );
        assert_eq!(
            page(5),
            page(6),
            "item 06 was left alone after {lines} lines"
        );
        split += (page(1) != page(6)) as usize;
    }
    assert!(split > 0, "the list never broke across a page");
}

/// Acceptance: `break-inside: avoid` keeps a list on one page, like any
/// other block.
#[test]
fn break_inside_avoid_keeps_a_list_on_one_page() {
    for lines in 20..48 {
        let output = lay_out(&filler_then_list(lines, 6), "ul { break-inside: avoid }");
        let page = |item: usize| run(&output, &format!("item {item:02}")).0;
        assert_eq!(page(1), page(6), "the list split after {lines} lines");
    }
}

/// Acceptance: a tight list has no space between its items, and a
/// loose list has the space the built-in sheet puts between its
/// paragraphs.
#[test]
fn a_loose_list_puts_space_between_its_items_and_a_tight_list_does_not() {
    let gap = |markdown: &str| {
        let output = lay_out(markdown, "");
        run(&output, "two").1.y - run(&output, "one").1.y
    };
    let (_, size) = indent_and_size();
    let line = 1.4 * size;
    assert!(
        close(gap("- one\n- two\n"), line),
        "{}",
        gap("- one\n- two\n")
    );
    assert!(
        close(gap("- one\n\n- two\n"), line + 0.5 * size),
        "{}",
        gap("- one\n\n- two\n")
    );
}

/// The text of a tight item is not in a `p` element, and the
/// paragraphs of a loose item are.
///
/// Every book ends in the footnote area, which is the box its notes
/// are set in and is no part of the list.
#[test]
fn only_a_loose_item_holds_p_elements() {
    let elements = |markdown: &str| -> Vec<&'static str> {
        let book = read(markdown);
        fleuron::style::defaults(&book, registry())
            .nodes()
            .iter()
            .map(|node| node.element)
            .collect()
    };
    assert_eq!(
        elements("- one\n- two\n"),
        ["book", "section", "ul", "li", "li", "notes"]
    );
    assert_eq!(
        elements("- one\n\n- two\n"),
        ["book", "section", "ul", "li", "p", "li", "p", "notes"]
    );
}

/// Acceptance: the fixture lays out to the same bytes twice, on the
/// same number of pages.
#[test]
fn two_layouts_of_the_fixture_are_byte_identical() {
    let once = lay_out(FIXTURE, "");
    let twice = lay_out(FIXTURE, "");
    assert_eq!(once.pages.len(), twice.pages.len());
    assert_eq!(
        serde_json::to_vec(&once).expect("a layout serializes"),
        serde_json::to_vec(&twice).expect("a layout serializes"),
    );
}

/// A PNG header and nothing else, two inches square at 96dpi.
struct Png;

impl ImageLoader for Png {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        (url == "image.png").then(|| {
            let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
            bytes.extend(13u32.to_be_bytes());
            bytes.extend(b"IHDR");
            bytes.extend(192u32.to_be_bytes());
            bytes.extend(192u32.to_be_bytes());
            bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
            bytes
        })
    }
}

/// A list that opens under an image the sheet anchors to the page,
/// and the box the image paints on the first page.
fn beside_an_image(css: &str) -> (LayoutOutput, (f32, f32, f32, f32)) {
    let items: String = (1..=8)
        .map(|item| format!("- item {item} {}\n", "words to wrap ".repeat(6)))
        .collect();
    let book = read(&format!("![an image](image.png)\n\n{items}"));
    let styles = styled(&book, css);
    let assets = Assets::probe(&book, &styles, &Png);
    let output = layout_book(&book, &styles, registry(), &assets);
    let image = output.pages[0]
        .items
        .iter()
        .find_map(|item| match item {
            DrawItem::Image { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .expect("the first page paints the image");
    (output, image)
}

/// A marker beside an image that its item wraps around keeps clear of
/// the image, and stays to the left of its item. This holds where the
/// image pushes the text over, and where the image reaches only the
/// marker.
#[test]
fn a_marker_keeps_clear_of_an_image_its_item_wraps_around() {
    let (indent, _) = indent_and_size();
    for left in [0.0, indent - 146.0] {
        let css = format!("img {{ position: absolute; top: 0; left: {left}pt; wrap-flow: end }}");
        let (output, (x, y, w, h)) = beside_an_image(&css);
        let page = runs(&output.pages[0]);
        let beside: Vec<&Run> = page
            .iter()
            .filter(|run| !run.named && run.y > y && run.y < y + h)
            .collect();
        assert!(
            !beside.is_empty(),
            "left {left}: no marker beside the image"
        );
        for marker in beside {
            assert!(
                marker.x >= x + w - 0.05,
                "left {left}: the marker at {} is over the image, which ends at {}",
                marker.x,
                x + w,
            );
            let item = page
                .iter()
                .filter(|run| run.named && close(run.y, marker.y))
                .map(|run| run.x)
                .fold(f32::MAX, f32::min);
            assert!(
                marker.right < item,
                "left {left}: the marker at {} runs into its item at {item}",
                marker.x,
            );
        }
    }
}

/// Every text run and rect of every page, one line each, in paint
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
                    DrawItem::Rect {
                        x, y, w, h, color, ..
                    } => Some(format!(
                        "rect {x:.2} {y:.2} {w:.2} {h:.2} {}",
                        color.to_hex()
                    )),
                    DrawItem::Image { .. }
                    | DrawItem::Background { .. }
                    | DrawItem::Rounded { .. } => None,
                })
                .collect()
        })
        .collect()
}

/// Acceptance: the display structure of the list fixture, under
/// snapshot.
#[test]
fn the_list_fixture_lays_out_to_the_display_list_it_is_checked_in_as() {
    insta::assert_json_snapshot!("list_fixture", described(&lay_out(FIXTURE, "")));
}
