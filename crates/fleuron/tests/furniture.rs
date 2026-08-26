//! Page furniture: running heads, folios, and the leaves that carry
//! neither.
//!
//! One fixture book runs all of it — front matter numbered in roman, a
//! body that restarts in arabic, a short chapter that leaves a blank
//! verso behind it — under an author sheet that asks for the furniture
//! a book has.

use fleuron::content::{Block, Book, HeadingLevel, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::pages::{DrawItem, Page, Side};
use fleuron::style::{PageQuery, Situation, Source, StyleTree, Stylesheets};

/// The author sheet the fixture is set under: roman front matter, an
/// arabic body, and running heads on the outer edge.
const FURNITURE_CSS: &str = r#"
/* Front matter takes a page of its own name, and numbers in roman. */
section:first-child { page: front }

@page front {
  @bottom-center { content: counter(page, lower-roman) }
}

/* The body restarts the folio: the chapter that opens it is page one. */
section:nth-child(2) { counter-reset: page 1 }

/* Running heads sit on the outer edge and name the chapter. */
@page :left  { @top-left  { content: string(chapter); font-size: 8pt } }
@page :right { @top-right { content: string(chapter); font-size: 8pt } }

/* A chapter's opening page carries neither head nor folio. */
@page chapter:first {
  @top-left { content: none }
  @top-right { content: none }
}
"#;

/// What the fixture's chapters are called, in reading order. The
/// running heads are checked against these.
const CHAPTERS: [&str; 4] = ["Preface", "Chapter One", "Chapter Two", "Chapter Three"];

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        position: None,
    }
}

fn heading(value: &str) -> Block {
    Block::Heading {
        id: NodeId::UNASSIGNED,
        level: HeadingLevel::H1,
        inlines: vec![text(value)],
        position: None,
    }
}

/// A paragraph of about `sentences` sentences of prose — enough of
/// them and the section runs past a page.
fn paragraph(sentences: usize) -> Block {
    let sentence = "The wind came off the water and the harbour lights \
                    went out one by one along the quay. ";
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![text(sentence.repeat(sentences).trim_end())],
        position: None,
    }
}

fn chapter(title: &str, paragraphs: usize, sentences: usize) -> Section {
    Section {
        id: NodeId::UNASSIGNED,
        source: None,
        title: None,
        blocks: std::iter::once(heading(title))
            .chain((0..paragraphs).map(|_| paragraph(sentences)))
            .collect(),
        position: None,
    }
}

/// The fixture: two pages of front matter, a chapter long enough to
/// turn several leaves, a chapter short enough to end on its own
/// opening recto, and a chapter after it.
fn fixture() -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![
            chapter(CHAPTERS[0], 8, 5),
            chapter(CHAPTERS[1], 20, 5),
            chapter(CHAPTERS[2], 1, 3),
            chapter(CHAPTERS[3], 6, 5),
        ],
    };
    book.assign_node_ids();
    book
}

fn styles(book: &Book) -> StyleTree {
    Stylesheets::parse(&[Source::author("furniture.css", FURNITURE_CSS)]).compile(book, registry())
}

fn pages(book: &Book, styles: &StyleTree) -> Vec<Page> {
    Paginator::new(registry(), styles).paginate(book)
}

/// One text item, flattened to what a reader of the page would see.
struct Item<'a> {
    text: &'a str,
    x: f32,
    y: f32,
    size: f32,
}

/// Everything painted outside the content box: the furniture, which
/// is what every test here is about. The content box is the master's,
/// so a page's own margins decide what counts as its margin.
fn furniture<'a>(page: &'a Page, styles: &StyleTree, named: Option<&str>) -> Vec<Item<'a>> {
    let geometry = styles
        .page(PageQuery {
            name: named,
            situation: Situation::Body(page.side),
        })
        .geometry;
    let (_, top) = geometry.content_origin();
    let bottom = top + geometry.content_size().1;
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                x, y, size, text, ..
            } if *y < top || *y > bottom => Some(Item {
                text,
                x: *x,
                y: *y,
                size: *size,
            }),
            _ => None,
        })
        .collect()
}

/// The first page each chapter opens on, by the heading it opens
/// with.
fn openings(pages: &[Page]) -> Vec<usize> {
    CHAPTERS
        .iter()
        .map(|title| {
            pages
                .iter()
                .position(|page| {
                    page.items.iter().any(
                        |item| matches!(item, DrawItem::Text { text, .. } if text.contains(title)),
                    )
                })
                .unwrap_or_else(|| panic!("{title} is somewhere in the book"))
        })
        .collect()
}

/// The chapter a page belongs to: the last one that had opened by the
/// time the page did.
fn chapter_of(openings: &[usize], page: usize) -> &'static str {
    let index = openings
        .iter()
        .rposition(|opening| *opening <= page)
        .expect("the book opens on its first chapter");
    CHAPTERS[index]
}

/// The head one page carries, when it carries one. Heads are the
/// furniture in the top margin.
fn head<'a>(page: &'a Page, styles: &StyleTree, named: Option<&str>) -> Option<Item<'a>> {
    let (_, top) = styles
        .page(PageQuery {
            name: named,
            situation: Situation::Body(page.side),
        })
        .geometry
        .content_origin();
    furniture(page, styles, named)
        .into_iter()
        .find(|item| item.y < top)
}

/// The folio one page carries: the furniture in the bottom margin.
fn folio(page: &Page, styles: &StyleTree, named: Option<&str>) -> Option<String> {
    let geometry = styles
        .page(PageQuery {
            name: named,
            situation: Situation::Body(page.side),
        })
        .geometry;
    let bottom = geometry.content_origin().1 + geometry.content_size().1;
    furniture(page, styles, named)
        .into_iter()
        .find(|item| item.y > bottom)
        .map(|item| item.text.to_string())
}

/// Running heads swap across the spread — verso to the left edge,
/// recto to the right — and name the chapter the page belongs to.
#[test]
fn running_heads_swap_across_a_chapter() {
    let book = fixture();
    let styles = styles(&book);
    let pages = pages(&book, &styles);
    let openings = openings(&pages);
    let chapter_one = openings[1];
    let chapter_two = openings[2];
    assert!(
        chapter_two - chapter_one >= 3,
        "the long chapter should turn several leaves",
    );

    let mut versos = 0;
    let mut rectos = 0;
    for (index, page) in pages.iter().enumerate().take(chapter_two).skip(chapter_one) {
        let Some(head) = head(page, &styles, Some("chapter")) else {
            assert_eq!(index, chapter_one, "only the opening page goes bare");
            continue;
        };
        assert_eq!(
            head.text,
            chapter_of(&openings, index),
            "page {} names the wrong chapter",
            page.number,
        );
        let geometry = styles
            .page(PageQuery {
                name: Some("chapter"),
                situation: Situation::Body(page.side),
            })
            .geometry;
        match page.side {
            Side::Verso => {
                versos += 1;
                assert!(
                    (head.x - geometry.margin.left).abs() < 1e-3,
                    "page {}: a verso head belongs on the left edge, not at {}",
                    page.number,
                    head.x,
                );
            }
            Side::Recto => {
                rectos += 1;
                assert!(
                    head.x > geometry.width / 2.0,
                    "page {}: a recto head belongs on the right edge, not at {}",
                    page.number,
                    head.x,
                );
            }
        }
    }
    assert!(versos > 0 && rectos > 0, "the chapter should span a spread");
}

/// Front matter numbers in roman; the body restarts at an arabic one.
#[test]
fn front_matter_is_roman_and_the_body_restarts_in_arabic() {
    let book = fixture();
    let styles = styles(&book);
    let pages = pages(&book, &styles);
    let body = openings(&pages)[1];
    assert!(body >= 2, "the front matter should run to two pages");

    let front: Vec<Option<String>> = pages[..body]
        .iter()
        .map(|page| folio(page, &styles, Some("front")))
        .collect();
    assert_eq!(
        front,
        vec![Some("i".to_string()), Some("ii".to_string())],
        "front matter should number in roman",
    );

    assert_eq!(
        pages[body].number, 1,
        "the body's first page should restart the folio",
    );
    assert_eq!(
        folio(&pages[body], &styles, Some("chapter")),
        None,
        "a chapter opening carries no folio",
    );
    assert_eq!(
        folio(&pages[body + 1], &styles, Some("chapter")),
        Some("2".to_string()),
        "the body should carry on in arabic",
    );
}

/// A chapter that opens on a recto after a short one leaves a blank
/// verso behind it: counted in the folio, and with nothing on it.
#[test]
fn a_short_chapter_leaves_a_counted_blank_verso() {
    let book = fixture();
    let styles = styles(&book);
    let pages = pages(&book, &styles);
    let openings = openings(&pages);
    let (short, next) = (openings[2], openings[3]);
    assert_eq!(
        next - short,
        2,
        "the short chapter should take one page and skip a verso",
    );

    let blank = &pages[short + 1];
    assert_eq!(blank.side, Side::Verso, "a blank leaf is never a recto");
    assert!(blank.items.is_empty(), "the blank leaf carries something");
    assert_eq!(
        pages[next].number,
        blank.number + 1,
        "the blank leaf should still be counted",
    );
    assert_eq!(
        pages[next].side,
        Side::Recto,
        "the next chapter opens on a recto",
    );
}

/// A page the flow put nothing on paints no furniture — even where
/// its master has a head and a folio to paint.
#[test]
fn a_page_with_no_content_paints_no_running_head() {
    let book = fixture();
    let styles = styles(&book);
    let pages = pages(&book, &styles);
    let blank = openings(&pages)[2] + 1;

    let master = styles.page(PageQuery {
        name: None,
        situation: Situation::Blank,
    });
    assert!(
        !master.boxes.is_empty(),
        "the blank's master should have furniture to decline to paint",
    );
    assert!(
        furniture(&pages[blank], &styles, None).is_empty(),
        "the blank leaf painted furniture",
    );
}

/// A running string is read as it stood when the page opened, not as
/// the first element on the page set it: a chapter opening mid-book
/// does not retitle the page it opens on. Books blind that page's head
/// for exactly this reason, which is what the fixture sheet does; this
/// is the same book with the blinding taken off.
#[test]
fn a_running_string_is_the_value_the_page_opened_with() {
    let book = fixture();
    let styles = Stylesheets::parse(&[Source::author(
        "unblinded.css",
        "section:nth-child(2) { counter-reset: page 1 }
         @page :left  { @top-left  { content: string(chapter); font-size: 8pt } }
         @page :right { @top-right { content: string(chapter); font-size: 8pt } }",
    )])
    .compile(&book, registry());
    let pages = pages(&book, &styles);
    let opening = openings(&pages)[2];

    assert_eq!(
        head(&pages[opening], &styles, Some("chapter")).map(|item| item.text.to_string()),
        Some(CHAPTERS[1].to_string()),
        "a chapter's opening page still carries the head it opened under",
    );
    assert_eq!(
        head(&pages[opening + 2], &styles, Some("chapter")).map(|item| item.text.to_string()),
        Some(CHAPTERS[2].to_string()),
        "the next page carries the chapter that had opened by its start",
    );
}

/// Snapshot of the furniture across a chapter boundary: the pages
/// either side of the short chapter, what each carries in its
/// margins, and what the blank leaf between them does not.
#[test]
fn furniture_at_a_chapter_boundary_snapshot() {
    let book = fixture();
    let styles = styles(&book);
    let pages = pages(&book, &styles);
    let short = openings(&pages)[2];
    let spread: Vec<_> = pages[short - 2..=short + 3]
        .iter()
        .map(|page| {
            let items: Vec<_> = furniture(page, &styles, Some("chapter"))
                .into_iter()
                .map(|item| {
                    serde_json::json!({
                        "text": item.text,
                        "x": item.x,
                        "y": item.y,
                        "size": item.size,
                    })
                })
                .collect();
            serde_json::json!({
                "number": page.number,
                "side": page.side,
                "furniture": items,
            })
        })
        .collect();
    insta::assert_json_snapshot!(spread);
}
