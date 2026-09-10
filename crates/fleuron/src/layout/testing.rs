//! The fixtures the layout tests are written against: books to
//! paginate, the built-in sheet's own answers, and the paint ops one
//! page came to.

use crate::LayoutOutput;
use crate::content::{Attributes, Block, Book, HeadingLevel, Inline, NodeId, Section, SourcePos};
use crate::fonts::FontRegistry;
use crate::pages::{DrawItem, Glyph, Page};
use crate::style::{Color, Content, MarginBox, PageQuery, PageStyle, Situation, StyleTree};

use super::{Paginator, layout_book};

pub(super) fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
}

pub(super) fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

pub(super) fn heading(value: &str) -> Block {
    Block::Heading {
        id: NodeId::UNASSIGNED,
        level: HeadingLevel::H1,
        inlines: vec![text(value)],
        attributes: Attributes::default(),
        position: Some(SourcePos { line: 1, column: 1 }),
        span: None,
    }
}

pub(super) fn paragraph(value: &str) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![text(value)],
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

pub(super) fn section(blocks: Vec<Block>) -> Section {
    Section {
        id: NodeId::UNASSIGNED,
        source: None,
        title: None,
        blocks,
        position: None,
        span: None,
    }
}

/// A book with its ids assigned: styling is keyed by node id, so
/// a tree that never had ids has no styles to look up.
pub(super) fn book_of(sections: Vec<Section>) -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections,
    };
    book.assign_node_ids();
    book
}

pub(super) fn paginate(sections: Vec<Section>) -> Vec<Page> {
    let book = book_of(sections);
    let styles = crate::style::defaults(&book, registry());
    Paginator::new(registry(), &styles).paginate(&book)
}

/// The built-in sheet's own answers, read back the way these
/// tests identify what they are looking at. Nothing here is a
/// constant: the sheet is asked.
pub(super) fn ua() -> &'static StyleTree {
    static STYLES: std::sync::OnceLock<StyleTree> = std::sync::OnceLock::new();
    STYLES.get_or_init(|| {
        let book = book_of(vec![section(vec![heading("H"), paragraph("prose")])]);
        crate::style::defaults(&book, registry())
    })
}

/// The font size the built-in sheet computes for one element.
pub(super) fn size_of(element: &str) -> f32 {
    let styles = ua();
    let node = styles
        .nodes()
        .iter()
        .find(|node| node.element == element)
        .unwrap_or_else(|| panic!("no {element} in the sample book"));
    styles.styles()[node.style as usize].font_size
}

pub(super) fn body_size() -> f32 {
    size_of("p")
}

pub(super) fn chapter_size() -> f32 {
    size_of("h1")
}

pub(super) fn folio_size() -> f32 {
    ua().default_page()
        .margin_box(MarginBox::BottomCenter)
        .expect("the default page has a folio")
        .style
        .font_size
}

/// The master a page of the fixture books resolves to.
pub(super) fn master(situation: Situation) -> &'static PageStyle {
    ua().page(PageQuery {
        name: Some("chapter"),
        situation,
    })
}

/// The same, with author CSS cascading over the built-in sheet.
pub(super) fn paginate_styled(css: &str, sections: Vec<Section>) -> Vec<Page> {
    let book = book_of(sections);
    let styles = styled(css, &book);
    Paginator::new(registry(), &styles).paginate(&book)
}

/// One book's styling with author CSS over the built-in sheet.
pub(super) fn styled(css: &str, book: &Book) -> StyleTree {
    crate::style::Stylesheets::parse(&[crate::style::Source::author("test.css", css)])
        .compile(book, registry())
}

/// The page box one sheet computes for a chapter page, which is
/// the master the fixture sections resolve to. The situation is
/// the page's own: mirrored margins put the columns of a verso
/// page at different offsets from a recto's.
pub(super) fn styled_geometry(css: &str, situation: Situation) -> crate::style::PageGeometry {
    let book = book_of(vec![section(vec![heading("H"), paragraph("prose")])]);
    styled(css, &book)
        .page(PageQuery {
            name: Some("chapter"),
            situation,
        })
        .geometry
}

/// The same for the page a flow put at `index`, which is where a
/// test that walks a book reads its columns from.
pub(super) fn page_geometry(css: &str, page: &Page) -> crate::style::PageGeometry {
    styled_geometry(css, Situation::Body(page.side))
}

/// Prose enough to break over several lines.
pub(super) fn prose() -> Block {
    paragraph(&"a quiet sentence of prose ".repeat(6))
}

/// Every filled rect one page paints, in paint order.
pub(super) fn rects(page: &Page) -> Vec<(f32, f32, f32, f32, Color)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rect { x, y, w, h, color } => Some((*x, *y, *w, *h, *color)),
            _ => None,
        })
        .collect()
}

pub(super) fn quote(blocks: Vec<Block>) -> Block {
    Block::Blockquote {
        id: NodeId::UNASSIGNED,
        blocks,
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

pub(super) fn scene_break() -> Block {
    Block::ThematicBreak {
        id: NodeId::UNASSIGNED,
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// One page's content paint ops, folio excluded: `(x, baseline,
/// size, text)` in paint order.
pub(super) fn content_items(page: &Page) -> Vec<(f32, f32, f32, &str)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                x, y, size, text, ..
            } if *size != folio_size() => Some((*x, *y, *size, text.as_str())),
            _ => None,
        })
        .collect()
}

/// One paint op of a content line: where it starts, the size it
/// is set at, and the text it was shaped from.
pub(super) type Run<'a> = (f32, f32, &'a str);

/// One content line: its baseline, and the runs sharing it.
pub(super) type ContentLine<'a> = (f32, Vec<Run<'a>>);

/// The content lines of one page: the paint ops grouped by the
/// baseline they share, in order down the page.
pub(super) fn content_lines(page: &Page) -> Vec<ContentLine<'_>> {
    let mut lines: Vec<ContentLine<'_>> = Vec::new();
    for (x, y, size, text) in content_items(page) {
        match lines.last_mut() {
            Some((baseline, runs)) if (*baseline - y).abs() < 1e-3 => runs.push((x, size, text)),
            _ => lines.push((y, vec![(x, size, text)])),
        }
    }
    lines
}

/// Where the content on one baseline ends: the far edge of the
/// last glyph on it, which is where a flush right edge falls.
pub(super) fn right_edge(page: &Page, baseline: f32) -> f32 {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Text {
                y,
                font_id,
                size,
                glyphs,
                ..
            } if *size != folio_size() && (*y - baseline).abs() < 1e-3 => {
                let last = glyphs.last()?;
                let upem = registry().metrics(*font_id)?.units_per_em as f32;
                let advance = registry().advance_width(*font_id, last.id)? as f32;
                Some(last.x + advance / upem * size)
            }
            _ => None,
        })
        .fold(f32::MIN, f32::max)
}

/// The content-box origin of the page `page` is.
pub(super) fn origin_of(page: &Page) -> (f32, f32) {
    master(Situation::Body(page.side)).geometry.content_origin()
}

/// Prose long enough to span several pages at the trade-paperback
/// measure.
pub(super) fn long_prose(paragraphs: usize) -> Vec<Block> {
    let words = "my father had a small estate in nottinghamshire his first inducements to travel ";
    (0..paragraphs)
        .map(|i| paragraph(&words.repeat(3 + i % 2)))
        .collect()
}

/// Total advance of a folio run, in points — for checking where
/// the centered run sits on the trim.
pub(super) fn run_width_pt(glyphs: &[Glyph], size: f32) -> f32 {
    let font = ua().root().font_id;
    let upem = registry().metrics(font).unwrap().units_per_em as f32;
    glyphs
        .iter()
        .map(|g| registry().advance_width(font, g.id).unwrap_or(0) as f32)
        .sum::<f32>()
        / upem
        * size
}

/// The folio painted on a page, if any: the text item at folio
/// size, read back as the digits it shapes.
pub(super) fn folio(page: &Page) -> Option<(&DrawItem, String)> {
    page.items.iter().find_map(|item| match item {
        DrawItem::Text {
            size,
            font_id,
            glyphs,
            ..
        } if *size == folio_size() => {
            let digits = glyphs
                .iter()
                .filter_map(|g| {
                    ('0'..='9').find(|c| registry().char_glyph(*font_id, *c) == Some(g.id))
                })
                .collect::<String>();
            Some((item, digits))
        }
        _ => None,
    })
}

/// True when the page's first paint op is a chapter heading.
pub(super) fn opens_a_chapter(page: &Page) -> bool {
    matches!(page.items.first(), Some(DrawItem::Text { size, .. }) if *size == chapter_size())
}

/// A chapter: a heading followed by enough prose to run over.
pub(super) fn chapter(title: &str, paragraphs: usize) -> Section {
    let mut blocks = vec![heading(title)];
    blocks.extend(long_prose(paragraphs));
    section(blocks)
}

/// Prose whose paragraphs are each set in a token of their own,
/// so a page's lines can be traced back to the paragraph they
/// were broken from. Lengths vary so page breaks land in every
/// position a paragraph has.
pub(super) fn tagged_prose(paragraphs: usize) -> Vec<Block> {
    (0..paragraphs)
        .map(|index| {
            let token = format!("p{index:02}");
            let words = vec![token; (5 + index % 11) * 18];
            paragraph(&words.join(" "))
        })
        .collect()
}

/// Which paragraph each of a page's lines came from, in order:
/// the token every word of that paragraph is set in.
pub(super) fn tagged_lines(page: &Page) -> Vec<String> {
    content_lines(page)
        .iter()
        .map(|(_, runs)| {
            runs[0]
                .2
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

/// The ornament the built-in sheet sets a thematic break in.
pub(super) fn ornament() -> String {
    let book = book_of(vec![section(vec![scene_break()])]);
    let styles = crate::style::defaults(&book, registry());
    let node = styles
        .nodes()
        .iter()
        .find(|node| node.element == "hr")
        .expect("the sample book has a thematic break");
    match &styles.styles()[node.style as usize].content {
        Content::Text(text) => text.clone(),
        other => panic!("the sheet sets a scene break in {other:?}"),
    }
}

/// A PNG header of the given pixel size, at the default 96dpi.
pub(super) fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(13u32.to_be_bytes());
    bytes.extend(b"IHDR");
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

/// A book laid out with one image in it, which the sheet can
/// anchor to the page. The image is 2in square at 96dpi.
pub(super) fn with_image(css: &str, sections: Vec<Section>) -> LayoutOutput {
    struct Png;
    impl crate::images::ImageLoader for Png {
        fn load(&self, url: &str) -> Option<Vec<u8>> {
            (url == "image.png").then(|| png(192, 192))
        }
    }
    let book = book_of(sections);
    let styles = styled(css, &book);
    let assets = crate::images::Assets::probe(&book, &Png);
    layout_book(&book, &styles, registry(), &assets)
}

/// The image the anchoring tests place.
pub(super) fn image() -> Block {
    Block::Image {
        id: NodeId::UNASSIGNED,
        url: "image.png".into(),
        alt: "a map of Lilliput".into(),
        attributes: Attributes::default(),
        position: Some(SourcePos { line: 3, column: 1 }),
        span: None,
    }
}

/// The images one page paints: `(x, y, width, height)`.
pub(super) fn painted(page: &Page) -> Vec<(f32, f32, f32, f32)> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Image { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
            _ => None,
        })
        .collect()
}

/// No paragraph is left a single line at either end of a page:
/// the run of lines a page sets of a paragraph it shares with
/// its neighbour is at least `widows` at the top and `orphans` at
/// the bottom.
pub(super) fn assert_orphans_and_widows(pages: &[Page], orphans: usize, widows: usize) {
    let tagged: Vec<Vec<String>> = pages.iter().map(tagged_lines).collect();
    assert_orphans_and_widows_over(&tagged, "page", orphans, widows);
}

/// The same over whatever the flow filled in order, which is
/// pages on an undivided page box and columns on a divided one.
pub(super) fn assert_orphans_and_widows_over(
    tagged: &[Vec<String>],
    what: &str,
    orphans: usize,
    widows: usize,
) {
    let mut boundaries = 0;
    for (index, lines) in tagged.iter().enumerate() {
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
            continue;
        };
        if index > 0 && tagged[index - 1].last() == Some(first) {
            let carried = lines.iter().take_while(|token| *token == first).count();
            assert!(
                carried >= widows,
                "{what} {}: {carried} line(s) of {first} carried over, widows is {widows}",
                index + 1,
            );
            boundaries += 1;
        }
        if tagged.get(index + 1).and_then(|next| next.first()) == Some(last) {
            let left = lines
                .iter()
                .rev()
                .take_while(|token| *token == last)
                .count();
            assert!(
                left >= orphans,
                "{what} {}: {left} line(s) of {last} left behind, orphans is {orphans}",
                index + 1,
            );
            boundaries += 1;
        }
    }
    assert!(
        boundaries >= 4,
        "only {boundaries} split paragraphs to check",
    );
}
