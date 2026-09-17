//! Links and the outline through the whole engine: markdown in, the
//! navigation of the display structure and the PDF out.
//!
//! Each test reads a manuscript the way the frontend reads one, lays it
//! out under the built-in sheet, and reads the links and the headings
//! back off the output and off the PDF.

use fleuron::LayoutOutput;
use fleuron::Warning;
use fleuron::content::{Book, LinkTarget};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::{DrawItem, Link, LinkTo, Navigation, OutlineEntry, PageBox};
use fleuron::style::{Source, Stylesheets};
use fleuron::wire;
use fleuron_markdown::Options;

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

/// A manuscript read the way the frontend reads one.
fn read(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "book.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn lay_out(book: &Book, css: &str) -> LayoutOutput {
    let styles = Stylesheets::parse(&[Source::author("book.css", css)]).compile(book, registry());
    layout_book(book, &styles, registry(), &Assets::none())
}

fn pdf(book: &Book, output: &LayoutOutput) -> Vec<u8> {
    fleuron::pdf::write(output, registry(), &Assets::none(), &book.metadata)
        .expect("the bundled face embeds")
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| *b as char).collect()
}

/// Paragraphs of prose, enough to push what follows onto later pages.
fn prose(paragraphs: usize) -> String {
    "It was the custom of the island that a stranger be kept at the gate until the \
     emperor's council had heard of him, and the council sat but twice a month.\n\n"
        .repeat(paragraphs)
}

/// The page index and the box of every run whose text holds `words`.
fn runs_of(output: &LayoutOutput, words: &str) -> Vec<(usize, f32, f32)> {
    output
        .pages
        .iter()
        .enumerate()
        .flat_map(|(index, page)| {
            page.items.iter().filter_map(move |item| match item {
                DrawItem::Text { text, x, y, .. } if text.contains(words) => Some((index, *x, *y)),
                _ => None,
            })
        })
        .collect()
}

/// Every link in the book, page by page.
fn links(output: &LayoutOutput) -> Vec<&Link> {
    output.pages.iter().flat_map(|page| &page.links).collect()
}

/// Every area of every link, page by page.
fn areas(output: &LayoutOutput) -> Vec<PageBox> {
    links(output)
        .into_iter()
        .flat_map(|link| link.areas.iter().copied())
        .collect()
}

/// The page a heading's words are set on.
fn page_of(output: &LayoutOutput, words: &str) -> u32 {
    runs_of(output, words)
        .first()
        .unwrap_or_else(|| panic!("{words:?} is not set"))
        .0 as u32
}

/// Acceptance: a cross-reference lands on the page its target is set
/// on, at the box the target takes there. An external link carries its
/// uri.
#[test]
fn a_link_goes_to_its_target_or_its_uri() {
    let markdown = format!(
        "# The Voyage\n\nSee [the hunter](#the-hunter) and [the society](https://example.com/society).\n\n\
         {}# The Hunter\n\nThe hunt began at dawn.\n",
        prose(30)
    );
    let book = read(&markdown);
    let output = lay_out(&book, "");
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let hunter = page_of(&output, "The Hunter");
    assert!(hunter > 0, "the target is on a later page");

    let links = links(&output);
    assert_eq!(links.len(), 2, "{links:?}");
    let LinkTo::Place { node, place } = links[0].to else {
        panic!("the cross-reference goes to {:?}", links[0].to);
    };
    assert_eq!(place.page, hunter);
    assert_eq!(
        book.anchors().resolve("#the-hunter", Some("book.md")),
        LinkTarget::Node(node)
    );
    let (_, x, y) = runs_of(&output, "The Hunter")[0];
    assert!(
        place.contains(x, y),
        "the heading at ({x}, {y}) is outside {place:?}"
    );
    assert_eq!(
        links[1].to,
        LinkTo::Uri("https://example.com/society".into())
    );

    let written = latin1(&pdf(&book, &output));
    assert_eq!(written.matches("/Subtype /Link").count(), 2, "{written}");
    assert!(written.contains("/URI (https://example.com/society)"));
    assert!(written.contains("/XYZ"), "no destination on a page");
}

/// Acceptance: a link broken across two lines is followed from both
/// lines and not from the space between them.
#[test]
fn a_link_broken_across_two_lines_is_an_area_on_each() {
    let words = "the long and winding account of the voyage to the island of the giants";
    let markdown = format!(
        "# One\n\nIt was the custom of the island that a stranger be kept at the gate, and so \
         the reader turns to [{words}](#two) before any other part.\n\n# Two\n\nThe end.\n"
    );
    let book = read(&markdown);
    let output = lay_out(&book, "");
    assert_eq!(links(&output).len(), 1, "the link is not one link");
    let areas = areas(&output);
    assert_eq!(areas.len(), 2, "the link is not on two lines: {areas:?}");
    let (first, second) = (areas[0], areas[1]);
    assert!(
        first.y + first.height <= second.y,
        "the areas reach into the space between the lines: {first:?} {second:?}"
    );

    let mut baselines: Vec<f32> = runs_of(&output, "")
        .into_iter()
        .filter(|(page, ..)| *page == 0)
        .map(|(.., y)| y)
        .collect();
    baselines.dedup();
    for area in [first, second] {
        let held = baselines
            .iter()
            .filter(|y| (area.y..=area.y + area.height).contains(*y))
            .count();
        assert_eq!(held, 1, "{area:?} does not hold one line of {baselines:?}");
    }

    let written = latin1(&pdf(&book, &output));
    assert_eq!(written.matches("/Subtype /Link").count(), 2);
}

/// A link that runs across runs of emphasis on one line is one area.
#[test]
fn a_link_with_emphasis_inside_is_one_area_on_one_line() {
    let book = read("# One\n\nSee [the *very* end](#one) here.\n");
    let output = lay_out(&book, "");
    assert_eq!(areas(&output).len(), 1, "{:?}", links(&output));
}

/// Acceptance: a link whose target nothing carries is set as text and
/// warns, naming the line and column it is written at.
#[test]
fn a_link_to_nothing_warns_and_is_set_as_text() {
    let book = read("# One\n\nSee\n[elsewhere](#nowhere) for the rest.\n");
    let output = lay_out(&book, "");
    assert!(links(&output).is_empty());
    assert_eq!(
        output.warnings,
        [Warning {
            message: "`#nowhere` names nothing in the book. The text is not a link.".into(),
            origin: Some("book.md:4:1".into()),
        }]
    );
    assert!(!runs_of(&output, "elsewhere").is_empty());
    assert!(!latin1(&pdf(&book, &output)).contains("/Annots"));
}

fn entry(entry: &OutlineEntry) -> (String, u8, Vec<(String, u8)>) {
    (
        entry.title.clone(),
        entry.level,
        entry
            .children
            .iter()
            .map(|child| (child.title.clone(), child.level))
            .collect(),
    )
}

/// Acceptance: the outline holds the headings nested by level, and each
/// entry points at its heading's place on its page.
#[test]
fn the_outline_nests_headings_by_level() {
    let markdown = format!(
        "# Part One\n\n## Chapter I\n\n{}### A Note\n\nNoted.\n\n## Chapter II\n\n{}# Part Two\n\nThe end.\n",
        prose(20),
        prose(20)
    );
    let book = read(&markdown);
    let output = lay_out(&book, "");
    let outline = &output.navigation.outline;
    assert_eq!(
        outline.iter().map(entry).collect::<Vec<_>>(),
        [
            (
                "Part One".into(),
                1,
                vec![("Chapter I".into(), 2), ("Chapter II".into(), 2)]
            ),
            ("Part Two".into(), 1, vec![]),
        ]
    );
    assert_eq!(outline[0].children[0].children[0].title, "A Note");
    let chapter = &outline[0].children[1];
    assert_eq!(chapter.place.page, page_of(&output, "Chapter II"));
    let (_, x, y) = runs_of(&output, "Chapter II")[0];
    assert!(chapter.place.contains(x, y));

    let written = latin1(&pdf(&book, &output));
    assert!(written.contains("/Type /Outlines"), "no outline in the PDF");
    assert!(written.contains("(Chapter II)"), "no entry for chapter II");
}

/// Acceptance: a book with no heading writes no outline.
#[test]
fn a_book_with_no_heading_writes_no_outline() {
    let book = read(&format!(
        "See [the society](https://example.com).\n\n{}",
        prose(2)
    ));
    let output = lay_out(&book, "");
    assert!(output.navigation.outline.is_empty());
    assert_eq!(links(&output).len(), 1);
    assert!(!latin1(&pdf(&book, &output)).contains("/Outlines"));
}

/// Acceptance: a book with no link and no heading writes the PDF it
/// wrote before links and the outline existed: no annotation, no
/// outline, and not one byte that differs from a run with no
/// navigation at all.
#[test]
fn a_book_with_no_link_and_no_heading_writes_the_same_pdf() {
    let book = read(&prose(12));
    let output = lay_out(&book, "");
    assert_eq!(output.navigation, Navigation::default());
    let written = pdf(&book, &output);
    let text = latin1(&written);
    assert!(!text.contains("/Annots") && !text.contains("/Outlines"));

    let mut bare = lay_out(&book, "");
    bare.navigation = Navigation::default();
    assert_eq!(written, pdf(&book, &bare));
}

/// Generated text inside a link is part of the link: a page reference
/// after it is followed as well.
#[test]
fn a_page_reference_after_a_link_is_part_of_it() {
    let markdown = format!(
        "# One\n\nSee [the end](#two).\n\n{}# Two\n\nThe end.\n",
        prose(30)
    );
    let book = read(&markdown);
    let css = "a::after { content: \" (page \" target-counter(attr(href url), page) \")\" }";
    let bare = lay_out(&book, "");
    let referred = lay_out(&book, css);
    let width = |output: &LayoutOutput| areas(output)[0].width;
    assert!(
        width(&referred) > width(&bare),
        "the page number is outside the link"
    );
}

/// Acceptance: a link with no text of its own is followed from the
/// text of its `::after`, which is how a contents entry prints its page
/// number.
#[test]
fn a_link_with_no_text_is_followed_from_its_after() {
    let markdown = format!(
        "# Contents\n\nThe Hunter [](#the-hunter)\n\n{}# The Hunter\n\nThe hunt began at dawn.\n",
        prose(30)
    );
    let book = read(&markdown);
    let css = "a::after { content: target-counter(attr(href url), page) }";
    let output = lay_out(&book, css);
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let links = links(&output);
    assert_eq!(links.len(), 1, "{links:?}");
    let number = (page_of(&output, "The hunt began") + 1).to_string();
    let printed = output.pages[0]
        .items
        .iter()
        .find_map(|item| match item {
            DrawItem::Text {
                text,
                x,
                y,
                pseudo_element: Some(_),
                ..
            } if *text == number => Some((*x, *y)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("page {number} is not printed"));
    assert_eq!(links[0].areas.len(), 1);
    assert!(
        links[0].contains(printed.0 + 0.5, printed.1 - 1.0),
        "the printed page number at {printed:?} is outside {:?}",
        links[0].areas
    );
    let (_, x, y) = runs_of(&output, "The Hunter")[0];
    assert!(
        !links[0].contains(x + 0.5, y - 1.0),
        "the words outside the link are in it"
    );
}

/// Acceptance: two links to one target nothing carries warn once, and
/// neither is on a page.
#[test]
fn links_to_one_missing_target_warn_once() {
    let book = read("# One\n\n[here](#nowhere) and [there](#nowhere).\n");
    let output = lay_out(&book, "");
    assert!(links(&output).is_empty());
    assert_eq!(output.warnings.len(), 1, "{:?}", output.warnings);
}

/// Acceptance: a book with no link has no link on any page.
#[test]
fn a_book_with_no_link_has_no_link_on_any_page() {
    let book = read(&format!("# One\n\n{}", prose(40)));
    let output = lay_out(&book, "");
    assert!(output.pages.len() > 1);
    assert!(output.pages.iter().all(|page| page.links.is_empty()));
}

/// The links a page carries cross the wire with it, and two runs write
/// the same bytes.
#[test]
fn links_cross_the_wire_and_two_runs_write_the_same_bytes() {
    let markdown = format!(
        "# One\n\nSee [the end](#two) or [the society](https://example.com).\n\n{}# Two\n\nThe end.\n",
        prose(30)
    );
    let book = read(&markdown);
    let once = wire::encode(&lay_out(&book, "")).expect("encodes");
    let twice = wire::encode(&lay_out(&book, "")).expect("encodes");
    assert_eq!(once, twice);
    let read = wire::decode(&once).expect("decodes");
    assert_eq!(read.pages[0].links.len(), 2);
}

/// Snapshot: the links of one page, with a cross-reference, an external
/// link and a link broken across two lines.
#[test]
fn the_links_of_a_page_snapshot() {
    let words = "the long and winding account of the voyage to the island of the giants";
    let markdown = format!(
        "# One\n\nSee [the society](https://example.com/society) first. It was the custom of \
         the island that a stranger be kept at the gate, and so the reader turns to \
         [{words}](#two) before any other part.\n\n{}# Two\n\nThe end.\n",
        prose(30)
    );
    let book = read(&markdown);
    let output = lay_out(&book, "");
    let page = &output.pages[0];
    assert_eq!(page.links.len(), 2);
    assert_eq!(page.links[1].areas.len(), 2, "{:?}", page.links[1]);
    insta::assert_json_snapshot!(page.links);
}
