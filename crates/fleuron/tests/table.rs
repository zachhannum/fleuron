//! Tables through the whole engine: markdown in, pages out.
//!
//! Each test reads a table the way the frontend reads one, lays it
//! out under the built-in sheet and a few rules of its own, and reads
//! the grid back off the display structure.

use fleuron::LayoutOutput;
use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::{Color, PageGeometry, PageQuery, Situation, Source, StyleTree, Stylesheets};
use fleuron_markdown::Options;

/// The fixture: the inventory of the man-mountain's pockets, a table
/// of eleven rows between two paragraphs.
const FIXTURE: &str = include_str!("../../../fixtures/tables.md");

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A manuscript read the way the frontend reads one.
fn read(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "tables.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn styled(book: &Book, css: &str) -> StyleTree {
    let styles = Stylesheets::parse(&[Source::author("tables.css", css)]).compile(book, registry());
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

/// The page box of the page a section opens on.
fn opening(css: &str) -> PageGeometry {
    let book = read("Prose.\n");
    styled(&book, css)
        .page(PageQuery {
            name: Some("chapter"),
            situation: Situation::First(Side::Recto),
        })
        .geometry
}

/// The size the built-in sheet sets body text in.
fn body_size() -> f32 {
    let book = read("Prose.\n");
    fleuron::style::defaults(&book, registry()).root().font_size
}

/// One run of text as a page paints it.
#[derive(Debug)]
struct Run {
    x: f32,
    right: f32,
    y: f32,
    size: f32,
    text: String,
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
                ..
            } if *size != folio => {
                let upem = registry().metrics(*font_id)?.units_per_em as f32;
                let right = glyphs
                    .last()
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
                })
            }
            _ => None,
        })
        .collect()
}

/// The first run anywhere in the book that says `text`.
fn run(output: &LayoutOutput, text: &str) -> Run {
    output
        .pages
        .iter()
        .flat_map(runs)
        .find(|run| run.text == text)
        .unwrap_or_else(|| panic!("nothing says {text:?}"))
}

/// Every filled rect one page paints, in paint order.
fn rects(page: &Page) -> Vec<(f32, f32, f32, f32, Color)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rect { x, y, w, h, color } => Some((*x, *y, *w, *h, *color)),
            _ => None,
        })
        .collect()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.05
}

/// Acceptance: `th:first-of-type { width: 8em }` sets that column to
/// 8em, and the two columns that name no width share the rest of the
/// measure equally.
#[test]
fn a_named_width_sets_its_column_and_the_rest_share_what_is_left() {
    let css = "th:first-of-type { width: 8em } th, td { padding: 0 }";
    let output = lay_out("| One | Two | Three |\n|---|---|---|\n| a | b | c |\n", css);
    let geometry = opening(css);
    let (left, _) = geometry.content_origin();
    let em = body_size();
    let share = (geometry.measure() - 8.0 * em) / 2.0;

    assert!(close(run(&output, "One").x, left));
    assert!(close(run(&output, "Two").x, left + 8.0 * em));
    assert!(close(run(&output, "Three").x, left + 8.0 * em + share));
    for (head, cell) in [("One", "a"), ("Two", "b"), ("Three", "c")] {
        assert!(close(run(&output, head).x, run(&output, cell).x), "{cell}");
    }
}

/// A width names the content of the cell, so the column also takes
/// the padding on either side of it.
#[test]
fn a_named_width_does_not_count_the_padding() {
    let css = "th:first-of-type { width: 80pt } th, td { padding: 0 6pt }";
    let output = lay_out("| One | Two |\n|---|---|\n| a | b |\n", css);
    let (left, _) = opening(css).content_origin();
    assert!(close(run(&output, "One").x, left + 6.0));
    assert!(close(run(&output, "Two").x, left + 6.0 + 80.0 + 6.0 + 6.0));
}

/// Acceptance: a cell whose text is longer than its column wraps
/// inside the column, and the row grows to hold it. The cell beside
/// it starts on the row's first line, and the next row starts below
/// the last.
#[test]
fn a_long_cell_wraps_inside_its_column_and_grows_the_row() {
    let long = "a sword of the length of five men, and on the right a bag or pouch \
                divided into two cells, each cell capable of holding three subjects";
    let css = "th, td { padding: 0 4pt }";
    let output = lay_out(
        &format!("| Short | Long |\n|---|---|\n| girdle | {long} |\n| fob | a watch |\n"),
        css,
    );
    let geometry = opening(css);
    let (left, _) = geometry.content_origin();
    let half = geometry.measure() / 2.0;
    let (from, to) = (left + half + 4.0, left + 2.0 * half - 4.0);

    let page = runs(&output.pages[0]);
    let lines: Vec<&Run> = page
        .iter()
        .filter(|run| long.contains(run.text.trim()) && run.x > left + half)
        .collect();
    assert!(lines.len() > 2, "the long cell did not wrap: {lines:?}");
    for line in &lines {
        assert!(line.x >= from - 0.05, "{line:?} starts left of its column");
        assert!(line.right <= to + 0.05, "{line:?} runs past its column");
    }
    let first = lines.first().expect("the long cell has lines").y;
    let last = lines.last().expect("the long cell has lines").y;
    assert!(
        close(run(&output, "girdle").y, first),
        "cells open on one line"
    );
    assert!(run(&output, "fob").y > last, "the next row starts below");
}

/// Acceptance: `tbody tr:nth-child(odd)` tints the first and third
/// body rows across the full width of the table, and leaves the
/// header row and the even rows alone.
#[test]
fn every_other_body_row_is_tinted_across_the_full_width() {
    let css = "tbody tr:nth-child(odd) { background-color: #e3e3e3 }";
    let output = lay_out(
        "| Head | Cell |\n|---|---|\n| r1 | a |\n| r2 | b |\n| r3 | c |\n| r4 | d |\n",
        css,
    );
    let geometry = opening(css);
    let (left, _) = geometry.content_origin();
    let tint = Color::rgb(0xe3, 0xe3, 0xe3);
    let tinted: Vec<(f32, f32, f32, f32, Color)> = rects(&output.pages[0])
        .into_iter()
        .filter(|rect| rect.4 == tint)
        .collect();
    assert_eq!(tinted.len(), 2, "{tinted:?}");
    for (x, _, w, _, _) in &tinted {
        assert!(close(*x, left));
        assert!(close(*w, geometry.measure()));
    }
    let covered = |word: &str| {
        let y = run(&output, word).y;
        tinted
            .iter()
            .any(|(_, top, _, h, _)| *top < y && y < top + h)
    };
    assert!(covered("r1") && covered("r3"));
    assert!(!covered("Head") && !covered("r2") && !covered("r4"));
}

/// Acceptance: a table taller than the page breaks between rows, the
/// header row is set again at the top of the next page, and every
/// body row is set once, whole, in order.
#[test]
fn a_table_taller_than_the_page_breaks_between_rows_and_repeats_its_header() {
    let mut markdown = String::from("| Pocket | Found |\n|---|---|\n");
    for row in 1..=60 {
        markdown.push_str(&format!("| row {row:02} | item {row:02} |\n"));
    }
    let output = lay_out(&markdown, "");
    assert!(output.pages.len() >= 2, "the table fitted on one page");

    let mut order = Vec::new();
    let mut headed = None;
    for page in &output.pages {
        let page = runs(page);
        let first = page.first().expect("a page of the table has text");
        assert_eq!(first.text, "Pocket", "a page opens without the header");
        match headed {
            None => headed = Some(first.y),
            Some(y) => assert!(close(first.y, y), "the header moved"),
        }
        for cell in page.iter().filter(|run| run.text.starts_with("row ")) {
            let beside = cell.text.replace("row", "item");
            assert!(
                page.iter()
                    .any(|other| other.text == beside && close(other.y, cell.y)),
                "{} was split from its row",
                cell.text,
            );
            order.push(cell.text.clone());
        }
    }
    let expected: Vec<String> = (1..=60).map(|row| format!("row {row:02}")).collect();
    assert_eq!(order, expected);

    // The header the manuscript wrote names the node it was read
    // from. Where it repeats, the engine wrote it, and it names none,
    // so the runs that name one node still tile it once.
    let origins: Vec<bool> = output
        .pages
        .iter()
        .map(|page| {
            page.items.iter().any(|item| {
                matches!(item, DrawItem::Text { text, origin, .. }
                    if text == "Pocket" && origin.is_some())
            })
        })
        .collect();
    assert!(origins[0], "the header the manuscript wrote names no node");
    assert!(
        origins[1..].iter().all(|named| !named),
        "a repeated header names a node: {origins:?}",
    );
}

/// A row taller than the page is set whole on one page anyway, and
/// the run says so.
#[test]
fn a_row_taller_than_the_page_is_set_whole_and_warns() {
    let long = "the searchers found a sort of engine ".repeat(40);
    let output = lay_out(
        &format!("| Where | What |\n|---|---|\n| pocket | {long} |\n"),
        "@page { size: 200pt 200pt; margin: 20pt }",
    );
    let warning = output
        .warnings
        .iter()
        .find(|warning| {
            warning
                .message
                .contains("table row is taller than the page")
        })
        .expect("a row taller than the page is worth saying");
    assert_eq!(warning.origin.as_deref(), Some("tables.md:3:1"));
    let pages: Vec<usize> = output
        .pages
        .iter()
        .enumerate()
        .filter(|(_, page)| runs(page).iter().any(|run| run.text.contains("engine")))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(pages.len(), 1, "the row was split over {pages:?}");
}

/// Acceptance: the alignment written in the delimiter row moves the
/// text of its column, and `td { text-align: right }` beats it.
#[test]
fn the_delimiter_alignment_moves_the_text_and_a_rule_beats_it() {
    let markdown = "| Left | Right |\n|:--|--:|\n| a | b |\n";
    let css = "th, td { padding: 0 4pt }";
    let geometry = opening(css);
    let (left, _) = geometry.content_origin();
    let half = geometry.measure() / 2.0;

    let written = lay_out(markdown, css);
    assert!(close(run(&written, "a").x, left + 4.0));
    assert!(close(run(&written, "b").right, left + 2.0 * half - 4.0));

    let right = lay_out(markdown, &format!("{css} td {{ text-align: right }}"));
    assert!(close(run(&right, "a").right, left + half - 4.0));
    assert!(close(run(&right, "b").right, left + 2.0 * half - 4.0));

    let flush = lay_out(markdown, &format!("{css} td {{ text-align: left }}"));
    assert!(close(run(&flush, "b").x, left + half + 4.0));
}

/// The rules one body row of two ruled cells paints, down the page:
/// the ones whose height is more than their width.
fn rules_down(output: &LayoutOutput) -> Vec<(f32, f32, f32, f32, Color)> {
    let mut down: Vec<(f32, f32, f32, f32, Color)> = rects(&output.pages[0])
        .into_iter()
        .filter(|(_, _, w, h, _)| h > w)
        .collect();
    down.sort_by(|one, other| one.0.total_cmp(&other.0));
    down
}

const RULED: &str = "table { border: none; margin: 0 } \
                     th { border: none; padding: 0 } \
                     td { border: 1pt solid; padding: 0 }";

const TWO_BY_TWO: &str = "| a | b |\n|---|---|\n| c | d |\n";

/// Collapsed, two cells that meet draw one rule between them, and
/// the text starts past the rule. Separated, each cell draws its own
/// border, so two rules stand side by side.
#[test]
fn collapsed_cells_share_a_rule_and_separated_cells_draw_two() {
    let (left, _) = opening(RULED).content_origin();

    let collapsed = lay_out(TWO_BY_TWO, RULED);
    let down = rules_down(&collapsed);
    assert_eq!(down.len(), 3, "{down:?}");
    assert!(down.iter().all(|rule| close(rule.2, 1.0)));
    assert!(close(run(&collapsed, "c").x, left + 1.0));

    let separate = lay_out(
        TWO_BY_TWO,
        &format!("{RULED} table {{ border-collapse: separate }}"),
    );
    let down = rules_down(&separate);
    assert_eq!(down.len(), 4, "{down:?}");
    assert!(close(down[1].0 + down[1].2, down[2].0), "{down:?}");
    assert!(close(run(&separate, "c").x, left + 1.0));
}

/// Acceptance: where two collapsed borders meet, the wider one draws
/// the rule. Of two as wide, the one on the left draws it.
#[test]
fn the_wider_border_draws_a_collapsed_rule_and_the_left_one_breaks_a_tie() {
    let red = Color::rgb(200, 0, 0);
    let blue = Color::rgb(0, 0, 200);
    let middle = |css: &str| {
        let output = lay_out(TWO_BY_TWO, &format!("{RULED} {css}"));
        let down = rules_down(&output);
        assert_eq!(down.len(), 3, "{down:?}");
        (down[1].2, down[1].4)
    };
    assert_eq!(
        middle(
            "td:first-child { border-right: 3pt solid rgb(200, 0, 0) } \
             td + td { border-left: 1pt solid rgb(0, 0, 200) }"
        ),
        (3.0, red),
    );
    assert_eq!(
        middle(
            "td:first-child { border-right: 1pt solid rgb(200, 0, 0) } \
             td + td { border-left: 2pt solid rgb(0, 0, 200) }"
        ),
        (2.0, blue),
    );
    assert_eq!(
        middle(
            "td:first-child { border-right: 1pt solid rgb(200, 0, 0) } \
             td + td { border-left: 1pt solid rgb(0, 0, 200) }"
        ),
        (1.0, red),
    );
}

/// Acceptance: a table that spans the columns of a two-column page
/// sets across the whole content box, and the columns resume under
/// it.
#[test]
fn a_spanning_table_sets_across_the_page_and_the_columns_resume_under_it() {
    let css = "@page { column-count: 2; column-gap: 18pt } table { column-span: all }";
    let above = "The officers searched the pockets of the man-mountain. ".repeat(4);
    let below = "They wrote down everything they found in a book. ".repeat(6);
    let markdown = format!(
        "{above}\n\n| Where | What |\n|---|---|\n| coat | handkerchief |\n| waistcoat | journal |\n\n\
         {below}\n\n{below}\n\n{below}\n"
    );
    let output = lay_out(&markdown, css);
    let geometry = opening(css);
    let (left, _) = geometry.content_origin();
    let second = geometry.column_origin(1).0;
    let page = &output.pages[0];

    // The rules over, inside and under the table run across the whole
    // content box, gutter and all. Each cell draws its own stretch of
    // a rule, so a rule is the rects that share a top.
    let mut across: Vec<(f32, f32, f32)> = Vec::new();
    for (x, y, w, h, _) in rects(page) {
        if w <= h {
            continue;
        }
        match across.iter_mut().find(|rule| close(rule.0, y)) {
            Some(rule) => {
                rule.1 = rule.1.min(x);
                rule.2 = rule.2.max(x + w);
            }
            None => across.push((y, x, x + w)),
        }
    }
    assert_eq!(across.len(), 3, "{across:?}");
    let right = left + geometry.content_size().0;
    for (y, from, to) in &across {
        assert!(
            close(*from, left) && close(*to, right),
            "the rule at {y} runs {from}..{to}, not {left}..{right}"
        );
    }
    let (top, bottom) = (across[0].0, across[2].0);

    // Above it, the prose stays in the first column. Under it, the
    // prose fills the first column and then the second.
    let runs = runs(page);
    assert!(
        runs.iter()
            .filter(|run| run.y < top)
            .all(|run| run.right <= left + geometry.measure() + 0.05)
    );
    let under: Vec<&Run> = runs.iter().filter(|run| run.y > bottom).collect();
    assert!(
        under.iter().any(|run| run.x < second),
        "nothing in the first column under it"
    );
    assert!(
        under.iter().any(|run| run.x >= second),
        "nothing in the second column under it"
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
                    DrawItem::Rect { x, y, w, h, color } => Some(format!(
                        "rect {x:.2} {y:.2} {w:.2} {h:.2} {}",
                        color.to_hex()
                    )),
                    DrawItem::Image { .. } | DrawItem::Background { .. } => None,
                })
                .collect()
        })
        .collect()
}

/// Acceptance: the display structure of the table fixture, under
/// snapshot.
#[test]
fn the_table_fixture_lays_out_to_the_display_list_it_is_checked_in_as() {
    let output = lay_out(FIXTURE, "");
    let sizes: Vec<f32> = output
        .pages
        .iter()
        .flat_map(runs)
        .map(|run| run.size)
        .collect();
    assert!(sizes.iter().all(|size| *size > 0.0));
    insta::assert_json_snapshot!("table_fixture", described(&output));
}
