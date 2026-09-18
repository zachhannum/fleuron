//! Code blocks through the whole engine: markdown in, pages out.
//!
//! Each test reads a code block the way the frontend reads one, lays it
//! out under the built-in sheet and a few rules of its own, and reads
//! the lines back off the display structure.

use fleuron::LayoutOutput;
use fleuron::content::{Block, Book};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Source, StyleTree, Stylesheets};
use fleuron_markdown::Options;

/// The fixture: an excerpt of a build manual, with a fenced block, an
/// indented block, inline code, and a listing longer than a page.
const FIXTURE: &str = include_str!("../../../fixtures/code.md");

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A manuscript read the way the frontend reads one.
fn read(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "code.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn styled(book: &Book, css: &str) -> StyleTree {
    let styles = Stylesheets::parse(&[Source::author("code.css", css)]).compile(book, registry());
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

/// One run of text as a page paints it.
#[derive(Debug, Clone)]
struct Run {
    /// Where the run starts, the spaces in front of its text
    /// included.
    x: f32,
    /// Where its first glyph that is not a space starts. A line of
    /// preformatted text carries its indentation as spaces, so this
    /// is where the reader sees the line begin.
    ink: f32,
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
                size,
                glyphs,
                ..
            } if *size != folio => {
                let at = text.len() - text.trim_start_matches(' ').len();
                let ink = glyphs
                    .iter()
                    .find(|glyph| glyph.range.start as usize >= at)
                    .map(|glyph| glyph.x)
                    .unwrap_or(*x);
                Some(Run {
                    x: *x,
                    ink,
                    y: *y,
                    size: *size,
                    text: text.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

/// Every run of the book, in reading order, with the page each one is
/// on.
fn all_runs(output: &LayoutOutput) -> Vec<(usize, Run)> {
    output
        .pages
        .iter()
        .enumerate()
        .flat_map(|(page, painted)| runs(painted).into_iter().map(move |run| (page, run)))
        .collect()
}

/// The text of every run of the book, in reading order.
fn texts(output: &LayoutOutput) -> Vec<String> {
    all_runs(output)
        .into_iter()
        .map(|(_, run)| run.text)
        .collect()
}

/// The lines of the fixture's shell listing, as the manuscript wrote
/// them.
fn listing(book: &Book) -> Vec<String> {
    let mut blocks = code_blocks(book);
    let (_, text) = blocks.pop().expect("the fixture has a listing");
    text.split('\n').map(String::from).collect()
}

/// Every code block of a book, as its info word and its text.
fn code_blocks(book: &Book) -> Vec<(Option<String>, String)> {
    book.sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            Block::CodeBlock { info, text, .. } => Some((info.clone(), text.clone())),
            _ => None,
        })
        .collect()
}

/// Acceptance: a fenced block in the fixture manuscript sets with its
/// line breaks intact, and the frontend emits no warning for it.
///
/// `read` fails the test on a warning, so reading the fixture is the
/// second half of the check.
#[test]
fn a_fenced_block_sets_as_the_lines_it_was_written_as() {
    let book = read(FIXTURE);
    let blocks = code_blocks(&book);
    assert_eq!(
        blocks
            .iter()
            .map(|(info, _)| info.as_deref())
            .collect::<Vec<_>>(),
        [Some("toml"), None, Some("sh")],
        "the fixture has a fenced block, an indented block, and a listing",
    );

    let output = lay_out(FIXTURE, "");
    assert!(
        output.warnings.is_empty(),
        "layout warned: {:?}",
        output.warnings
    );
    let painted = texts(&output);
    for line in blocks[0].1.split('\n').filter(|line| !line.is_empty()) {
        assert!(
            painted.iter().any(|run| run == line),
            "the fenced block lost {line:?}:\n{painted:#?}",
        );
    }
}

/// Acceptance: indentation is preserved to the character. Two lines
/// indented by the same number of spaces start at the same place, and a
/// line indented by more starts further in, by the width of the spaces
/// between them.
#[test]
fn indentation_is_preserved_to_the_character() {
    let markdown = "# C\n\n```\nzero\n  two\n  two again\n    four\n```\n";
    let output = lay_out(markdown, "");
    let at = |text: &str| {
        all_runs(&output)
            .into_iter()
            .find(|(_, run)| run.text == text)
            .unwrap_or_else(|| panic!("nothing says {text:?}"))
            .1
    };
    let (zero, two, again, four) = (at("zero"), at("  two"), at("  two again"), at("    four"));
    assert_eq!(two.ink, again.ink, "two lines of one indent start apart");
    let step = two.ink - zero.ink;
    assert!(step > 0.0, "the indent took no room");
    assert!(
        (four.ink - two.ink - step).abs() < 0.05,
        "four spaces are not twice two: {} and {}",
        four.ink - two.ink,
        step,
    );
    assert_eq!(zero.x, four.x, "the lines start at different places");
}

/// Acceptance: nothing in a code block is justified, whatever the
/// cascade asks for around it.
///
/// The fixture is set twice, once under the built-in sheet and once
/// under a sheet that justifies the whole book. Every line of every
/// code block comes out at the same place and the same width, which is
/// what justifying nothing means.
#[test]
fn a_code_block_is_not_justified() {
    let book = read(FIXTURE);
    let flush = lay_out(FIXTURE, "");
    let filled = lay_out(FIXTURE, "book { text-align: justify }");
    let lines: Vec<String> = code_blocks(&book)
        .iter()
        .flat_map(|(_, text)| text.split('\n').map(str::to_string).collect::<Vec<_>>())
        .filter(|line| !line.trim().is_empty())
        .collect();

    let set = |output: &LayoutOutput| -> Vec<String> {
        all_runs(output)
            .into_iter()
            .filter(|(_, run)| lines.contains(&run.text))
            .map(|(page, run)| format!("{page} {:.2} {:.2} {:?}", run.x, run.y, run.text))
            .collect()
    };
    let flush = set(&flush);
    assert_eq!(
        flush.len(),
        lines.len(),
        "not every line was found: {flush:#?}"
    );
    assert_eq!(flush, set(&filled));
}

/// Acceptance: nothing inside a code block hyphenates, whatever the
/// cascade asks for around it.
///
/// The line is longer than the measure and made of words the patterns
/// break, so a paragraph of the same words hyphenates. The code block
/// runs past the measure instead, with no hyphen anywhere on it.
#[test]
fn a_code_block_does_not_hyphenate() {
    let words = "extraordinarily unaccountable circumstances extraordinarily unaccountable \
                 circumstances extraordinarily unaccountable circumstances";
    let css = "book { hyphens: auto; text-align: justify }";
    let prose = lay_out(&format!("# C\n\n{words}\n"), css);
    assert!(
        texts(&prose).iter().any(|run| run.ends_with('-')),
        "the sheet did not reach the prose, so the block proves nothing",
    );

    let code = lay_out(&format!("# C\n\n```\n{words}\n```\n"), css);
    let painted = texts(&code);
    assert!(
        painted.iter().any(|run| run == words),
        "the code line broke: {painted:#?}",
    );
    assert!(
        !painted.iter().any(|run| run.ends_with('-')),
        "a line of the code block ends in a hyphen: {painted:#?}",
    );
}

/// Acceptance: a block longer than a page breaks and keeps its shape.
/// The listing crosses a page, and every line of it is on a page in the
/// order it was written, indentation and all.
#[test]
fn a_block_longer_than_a_page_breaks_and_keeps_its_shape() {
    let book = read(FIXTURE);
    let output = lay_out(FIXTURE, "");
    let lines = listing(&book);
    let painted = all_runs(&output);

    let mut pages = Vec::new();
    let mut from = 0;
    for line in lines.iter().filter(|line| !line.trim().is_empty()) {
        let set = line.trim_end();
        let at = painted[from..]
            .iter()
            .position(|(_, run)| run.text == set)
            .unwrap_or_else(|| panic!("the listing lost {set:?} after run {from}"));
        from += at + 1;
        pages.push(painted[from - 1].0);
    }
    let first = *pages.first().expect("the listing has lines");
    let last = *pages.last().expect("the listing has lines");
    assert!(
        last > first,
        "the listing fits on one page, so it never broke"
    );
    assert!(pages.windows(2).all(|pair| pair[0] <= pair[1]));
}

/// Part: a line wider than the measure runs past it, and the engine
/// warns naming the line and column the block was written at.
#[test]
fn a_line_wider_than_the_measure_overflows_and_warns() {
    let wide = "x".repeat(400);
    let output = lay_out(&format!("# C\n\n```\nnarrow\n{wide}\n```\n"), "");
    let warnings: Vec<&str> = output
        .warnings
        .iter()
        .map(|warning| warning.message.as_str())
        .collect();
    assert_eq!(
        warnings,
        [concat!(
            "A line of a code block is wider than the measure. It runs past it, ",
            "because a code block breaks only where its own text does.",
        )],
    );
    assert_eq!(
        output.warnings[0].origin.as_deref(),
        Some("code.md:3:1"),
        "the warning does not name where the block was written",
    );
    assert!(
        texts(&output).iter().any(|run| run == &wide),
        "the wide line broke rather than overflowed",
    );
}

/// Part: `pre` reaches the style tree as an element, so a rule on it
/// reaches a code block and nothing else.
#[test]
fn a_rule_on_pre_reaches_a_code_block() {
    let markdown = "# C\n\nProse.\n\n```\ncode\n```\n";
    let output = lay_out(markdown, "pre { font-size: 20pt }");
    let at = |text: &str| {
        all_runs(&output)
            .into_iter()
            .find(|(_, run)| run.text == text)
            .unwrap_or_else(|| panic!("nothing says {text:?}"))
            .1
    };
    assert_eq!(at("code").size, 20.0);
    assert_ne!(at("Prose.").size, 20.0);
}

/// The built-in sheet asks for a monospace family. No bundled face is
/// monospace, so a code block sets in the bundled family until a book
/// declares one with `@font-face`.
#[test]
fn a_code_block_asks_for_a_monospace_family() {
    let book = read("# C\n\n```\ncode\n```\n");
    let styles = fleuron::style::defaults(&book, registry());
    let node = styles
        .nodes()
        .iter()
        .find(|node| node.element == "pre")
        .expect("the book has a code block");
    let family = &styles.styles()[node.style as usize].font_family;
    assert_eq!(format!("{family:?}"), "[Generic(Monospace)]");
}

/// Layout is deterministic over the fixture: two runs, byte-identical
/// output.
#[test]
fn laying_out_the_code_fixture_twice_gives_the_same_pages() {
    let once = lay_out(FIXTURE, "");
    let twice = lay_out(FIXTURE, "");
    assert_eq!(
        serde_json::to_string(&once).expect("the output serializes"),
        serde_json::to_string(&twice).expect("the output serializes"),
    );
}

/// What the display structure of one book comes to, run by run, in
/// paint order.
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

/// Acceptance: the display structure of the code fixture, under
/// snapshot.
#[test]
fn the_code_fixture_lays_out_to_the_display_list_it_is_checked_in_as() {
    insta::assert_json_snapshot!("code_fixture", described(&lay_out(FIXTURE, "")));
}
