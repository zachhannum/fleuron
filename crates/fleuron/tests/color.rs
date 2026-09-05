//! Colour: what a sheet sets, what the runs carry, and what a page
//! comes out as when nothing sets any.
//!
//! One fixture — a chapter opening and a paragraph under it — set
//! twice: once under a sheet that names no colour, and once under one
//! that colours the section and the heading in it.

use fleuron::content::{Block, Book, HeadingLevel, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Color, Source, StyleTree, Stylesheets};

/// A sheet that sets a page and a heading and names no colour.
const PLAIN_CSS: &str = r#"
@page {
  size: 240pt 180pt;
  margin: 24pt;
  @bottom-center { content: counter(page); font-size: 8pt }
}

h1 { font-size: 14pt }
"#;

/// The same sheet, with the section in a grey the prose inherits, the
/// heading in a red of its own, and the emphasis in a third colour.
const COLOURED_CSS: &str = r#"
@page {
  size: 240pt 180pt;
  margin: 24pt;
  @bottom-center { content: counter(page); font-size: 8pt; color: #808080 }
}

h1 { font-size: 14pt; color: rgb(180, 30, 30) }
section { color: #444 }
em { color: darkslateblue }
"#;

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

/// The fixture: a chapter opening, and a paragraph with an emphasised
/// phrase in it — a run the sheet can colour on its own.
fn fixture() -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![
                Block::Heading {
                    id: NodeId::UNASSIGNED,
                    level: HeadingLevel::H1,
                    inlines: vec![text("The Quay")],
                    position: None,
                },
                Block::Paragraph {
                    id: NodeId::UNASSIGNED,
                    inlines: vec![
                        text("The wind came off the water and the "),
                        Inline::Emphasis {
                            id: NodeId::UNASSIGNED,
                            children: vec![text("harbour lights")],
                            position: None,
                        },
                        text(" went out."),
                    ],
                    position: None,
                },
            ],
            position: None,
        }],
    };
    book.assign_node_ids();
    book
}

/// A measure narrow enough that the emphasised word has to break,
/// and an emphasis with a colour of its own.
const HYPHENATED_CSS: &str = r#"
@page { size: 130pt 200pt; margin: 12pt }

p { hyphens: auto }
em { color: rgb(180, 30, 30) }
"#;

/// One paragraph whose every word is emphasised, so a break inside a
/// word falls inside the colour.
fn hyphenated() -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![Block::Paragraph {
                id: NodeId::UNASSIGNED,
                inlines: vec![Inline::Emphasis {
                    id: NodeId::UNASSIGNED,
                    children: vec![text(
                        "The extraordinarily inconsiderate lamplighter considered \
                         the incomprehensible correspondence.",
                    )],
                    position: None,
                }],
                position: None,
            }],
            position: None,
        }],
    };
    book.assign_node_ids();
    book
}

fn styles(book: &Book, css: &str) -> StyleTree {
    Stylesheets::parse(&[Source::author("colour.css", css)]).compile(book, registry())
}

fn pages(book: &Book, styles: &StyleTree) -> Vec<Page> {
    Paginator::new(registry(), styles).paginate(book)
}

/// One draw item, every field of it, on one line. The match names
/// each field rather than eliding it, so a field added to the display
/// structure has to be answered for here.
fn described(item: &DrawItem) -> String {
    match item {
        DrawItem::Text {
            x,
            y,
            font_id,
            size,
            text,
            source,
            source_map,
            features,
            // What every other test here is about; this one is about
            // everything else standing where it stood.
            color: _,
            glyphs,
        } => {
            let placed: Vec<String> = glyphs
                .iter()
                .map(|glyph| {
                    format!(
                        "{}@{:?}:{}..{}",
                        glyph.id, glyph.x, glyph.range.start, glyph.range.end
                    )
                })
                .collect();
            format!(
                "text {x:?} {y:?} font {font_id} at {size:?}pt {text:?} source {source:?} {source_map:?} \
                 small-caps {} glyphs {}",
                features.small_caps,
                placed.join(" ")
            )
        }
        DrawItem::Rect {
            x,
            y,
            w,
            h,
            color: _,
        } => format!("rect {x:?} {y:?} {w:?} {h:?}"),
        DrawItem::Image { x, y, w, h, asset } => {
            format!("image {x:?} {y:?} {w:?} {h:?} asset {asset}")
        }
    }
}

/// Every page of a laid-out book, as the lines a painter would read.
fn description(pages: &[Page]) -> Vec<Vec<String>> {
    pages
        .iter()
        .map(|page| {
            std::iter::once(format!(
                "page {} {:?} {:?}x{:?}",
                page.number, page.side, page.width, page.height
            ))
            .chain(page.items.iter().map(described))
            .collect()
        })
        .collect()
}

/// The colour of every run whose text begins with a given word.
fn colour_of(pages: &[Page], opening: &str) -> Vec<Color> {
    pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { text, color, .. } if text.starts_with(opening) => Some(*color),
            _ => None,
        })
        .collect()
}

/// Every run carries the colour its own style named: the heading its
/// own, the prose the section's, the emphasis inside the prose a
/// third, and the folio what the margin box was given.
#[test]
fn every_run_carries_the_colour_its_style_named() {
    let book = fixture();
    let styles = styles(&book, COLOURED_CSS);
    let pages = pages(&book, &styles);
    assert_eq!(colour_of(&pages, "The Quay"), [Color::rgb(180, 30, 30)]);
    assert_eq!(colour_of(&pages, "The wind"), [Color::rgb(68, 68, 68)]);
    assert_eq!(colour_of(&pages, "harbour"), [Color::rgb(72, 61, 139)]);
    assert_eq!(colour_of(&pages, "1"), [Color::rgb(128, 128, 128)]);
}

/// A hyphen the breaker draws inside a coloured word is the word's
/// colour. It is set in the paragraph's face, because that is where
/// its width was charged, and colour costs no width.
#[test]
fn a_hyphen_inside_a_coloured_word_takes_the_words_colour() {
    let book = hyphenated();
    let styles = styles(&book, HYPHENATED_CSS);
    let pages = pages(&book, &styles);
    let hyphens: Vec<Color> = pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { text, color, .. } if text.ends_with('-') => Some(*color),
            _ => None,
        })
        .collect();
    assert!(!hyphens.is_empty(), "no line was hyphenated");
    assert!(
        hyphens
            .iter()
            .all(|color| *color == Color::rgb(180, 30, 30)),
        "a hyphen came out in {hyphens:?}"
    );
}

/// A book whose sheet names no colour is set the way it was set
/// before a sheet could name one: the pages, the runs and every glyph
/// on them stand where they stood.
#[test]
fn a_sheet_that_names_no_colour_sets_the_book_unchanged() {
    let book = fixture();
    let styles = styles(&book, PLAIN_CSS);
    let pages = pages(&book, &styles);
    let painted: Vec<Color> = pages
        .iter()
        .flat_map(|page| &page.items)
        .map(|item| match item {
            DrawItem::Text { color, .. } | DrawItem::Rect { color, .. } => *color,
            DrawItem::Image { .. } => Color::BLACK,
        })
        .collect();
    assert!(
        painted.iter().all(|color| *color == Color::BLACK),
        "something was painted in {painted:?}"
    );
    insta::assert_json_snapshot!("a_book_no_sheet_coloured", description(&pages));
}
