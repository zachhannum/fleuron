//! Layout: box construction, inline layout, fragmentation.
//!
//! ```text
//! content + style ─► box tree ─► line layout ─► fragmentation ─► pages
//! ```
//!
//! v0.1 folds the middle of the pipeline into one pass: each section
//! becomes its lines (headings with the chapter style, paragraphs with
//! the body style, via `lines::LineLayout`), and the paginator flows
//! those lines into page content boxes. The style compiler (#7) will
//! interpose a real box tree between content and line layout; the
//! flow below is the fragmentation contract it must satisfy.

use crate::LayoutOutput;
use crate::content::{Block, Book, Section};
use crate::fonts::FontRegistry;
use crate::lines::{Line, LineBreakOptions, LineLayout, ParagraphStyle};
use crate::pages::{DrawItem, Glyph, Page, Side};

/// Page and content-box geometry, in points. v0.1: hardcoded 6×9in
/// trade-book defaults; page masters (#15) and `@page` (#6) both
/// generalize this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    /// Trimmed page width.
    pub width: f32,
    /// Trimmed page height.
    pub height: f32,
    /// Margin on the spine side: left on recto, right on verso.
    pub inner: f32,
    /// Margin on the outer edge.
    pub outer: f32,
    /// Margin at the top of the page.
    pub top: f32,
    /// Margin at the bottom of the page.
    pub bottom: f32,
}

impl PageGeometry {
    /// 6×9in with the margins of a trade paperback, mirrored across
    /// the spread.
    pub fn trade_paperback() -> PageGeometry {
        PageGeometry {
            width: 432.0,  // 6in
            height: 648.0, // 9in
            inner: 54.0,
            outer: 42.0,
            top: 54.0,
            bottom: 54.0,
        }
    }

    /// Origin (top-left) of the content box on one side, in page
    /// coordinates.
    pub fn content_origin(self, side: Side) -> (f32, f32) {
        let x = match side {
            Side::Recto => self.inner,
            Side::Verso => self.outer,
        };
        (x, self.top)
    }

    /// Size of the content box; identical on both sides, since the
    /// margins mirror.
    pub fn content_size(self) -> (f32, f32) {
        (
            self.width - self.inner - self.outer,
            self.height - self.top - self.bottom,
        )
    }

    /// The measure line layout breaks to: the content box width.
    pub fn measure(self) -> f32 {
        self.content_size().0
    }
}

/// One book through the whole pipeline: lines laid out, flowed into
/// pages, everything the output needs assembled.
pub fn layout_book(book: &Book, registry: &FontRegistry) -> LayoutOutput {
    let paginator = Paginator::new(registry, PageGeometry::trade_paperback());
    LayoutOutput {
        pages: paginator.paginate(book),
        fonts: (0..registry.len() as u16)
            .filter_map(|id| registry.font_ref(id).cloned())
            .collect(),
        warnings: Vec::new(),
    }
}

/// The pagination pass: laid-out lines in, `Page`s of `DrawItem`s out.
///
/// Flow rules (v0.1): lines stack from the top of the content box; a
/// line that does not fit starts the next page (a line taller than a
/// whole page overflows rather than disappears); a chapter starts a
/// fresh recto page, with a blank verso inserted when the flow sits
/// on one.
pub struct Paginator<'a> {
    registry: &'a FontRegistry,
    geometry: PageGeometry,
    lines: LineLayout<'a>,
}

impl<'a> Paginator<'a> {
    pub fn new(registry: &'a FontRegistry, geometry: PageGeometry) -> Self {
        Paginator {
            registry,
            geometry,
            lines: LineLayout::new(registry),
        }
    }

    /// Flows one book into numbered, side-tagged pages.
    pub fn paginate(&self, book: &Book) -> Vec<Page> {
        let mut pages = Vec::new();
        for section in &book.sections {
            self.flow_section(section, &mut pages);
        }
        for (index, page) in pages.iter_mut().enumerate() {
            page.number = index as u32 + 1;
            page.side = Side::of_number(page.number);
        }
        pages
    }

    /// Lays a section out and flows it, opening on a fresh recto.
    fn flow_section(&self, section: &Section, pages: &mut Vec<Page>) {
        let mut flow: Vec<Line> = Vec::new();
        self.append_blocks(&section.blocks, ParagraphStyle::BODY, &mut flow);
        if flow.is_empty() {
            return;
        }
        // A chapter opens on a recto: when the next natural page is a
        // verso, it ships blank.
        if pages.len() % 2 == 1 {
            pages.push(Page {
                number: 0,
                side: Side::Verso,
                items: Vec::new(),
            });
        }

        let (_, content_h) = self.geometry.content_size();
        let mut cursor = 0f32;
        let mut items = Vec::new();

        for line in flow {
            if !items.is_empty() && cursor + line.box_.height > content_h {
                self.push_page(pages, std::mem::take(&mut items));
                cursor = 0.0;
            }
            let (x, y) = self.geometry.content_origin(self.current_side(pages));
            let baseline = y + cursor + line.box_.baseline;
            items.append(&mut self.text_items(&line, x, baseline));
            cursor += line.box_.height;
        }
        if !items.is_empty() {
            self.push_page(pages, items);
        }
    }

    /// A section's blocks as laid-out lines, in document order.
    fn append_blocks(&self, blocks: &[Block], _container: ParagraphStyle, flow: &mut Vec<Line>) {
        for block in blocks {
            match block {
                Block::Heading { inlines, .. } => {
                    let style = ParagraphStyle::CHAPTER;
                    let measure = self.geometry.measure();
                    flow.extend(self.lines.layout(
                        inlines,
                        style,
                        measure,
                        LineBreakOptions::default(),
                    ));
                }
                Block::Paragraph { inlines, .. } => {
                    let style = ParagraphStyle::BODY;
                    let measure = self.geometry.measure();
                    flow.extend(self.lines.layout(
                        inlines,
                        style,
                        measure,
                        LineBreakOptions::default(),
                    ));
                }
                // v0.1 lays out prose; the block-remainder blocks
                // arrive with the style compiler (#7) and image
                // sizing (#5, v0.2).
                Block::Blockquote { .. } | Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }

    fn push_page(&self, pages: &mut Vec<Page>, items: Vec<DrawItem>) {
        pages.push(Page {
            number: 0,
            side: self.current_side(pages),
            items,
        });
    }

    /// The side of the page under construction: it will sit at index
    /// `pages.len()`, so its number is `pages.len() + 1`.
    fn current_side(&self, pages: &[Page]) -> Side {
        Side::of_number(pages.len() as u32 + 1)
    }

    /// One line as paint ops: every run a `DrawItem::Text` at the
    /// baseline, glyphs placed at their accumulated advances.
    fn text_items(&self, line: &Line, x: f32, baseline: f32) -> Vec<DrawItem> {
        let mut items = Vec::new();
        let mut x_cursor = x;
        for run in &line.runs {
            let upem = self
                .registry
                .metrics(run.font_id)
                .map(|m| m.units_per_em as f32)
                .unwrap_or(1000.0);
            let mut glyphs = Vec::with_capacity(run.glyphs.len());
            let mut glyph_x = x_cursor;
            for shaped in &run.glyphs {
                glyphs.push(Glyph {
                    id: shaped.id,
                    x: glyph_x,
                });
                glyph_x += shaped.x_advance as f32 / upem * run.size;
            }
            items.push(DrawItem::Text {
                x: x_cursor,
                y: baseline,
                font_id: run.font_id,
                size: run.size,
                glyphs,
            });
            x_cursor = glyph_x;
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{HeadingLevel, Inline, NodeId, SourcePos};

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
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
            position: Some(SourcePos { line: 1, column: 1 }),
        }
    }

    fn paragraph(value: &str) -> Block {
        Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![text(value)],
            position: None,
        }
    }

    fn section(blocks: Vec<Block>) -> Section {
        Section {
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks,
            position: None,
        }
    }

    fn paginate(sections: Vec<Section>) -> Vec<Page> {
        let book = Book {
            metadata: Default::default(),
            sections,
        };
        Paginator::new(registry(), PageGeometry::trade_paperback()).paginate(&book)
    }

    /// Prose long enough to span several pages at the trade-paperback
    /// measure.
    fn long_prose(paragraphs: usize) -> Vec<Block> {
        let words =
            "my father had a small estate in nottinghamshire his first inducements to travel ";
        (0..paragraphs)
            .map(|i| paragraph(&words.repeat(3 + i % 2)))
            .collect()
    }

    /// 6×9in trim with mirrored margins; the content box is what
    /// remains.
    #[test]
    fn trade_paperback_geometry() {
        let geometry = PageGeometry::trade_paperback();
        assert_eq!(geometry.width, 432.0);
        assert_eq!(geometry.height, 648.0);
        let (w, h) = geometry.content_size();
        assert_eq!(w, 336.0);
        assert_eq!(h, 540.0);
        assert_eq!(geometry.measure(), 336.0);
        // Mirrored: the spine sits `inner` from whichever edge the
        // page's side puts it on, and the content box is the same
        // size on both sides.
        let (recto_x, top) = geometry.content_origin(Side::Recto);
        let (verso_x, verso_top) = geometry.content_origin(Side::Verso);
        assert_eq!(recto_x, 54.0);
        assert_eq!(verso_x, 42.0);
        assert_eq!(verso_x + w, geometry.width - geometry.inner);
        assert_eq!(top, verso_top);
    }

    /// Odd pages are recto, even pages verso — books open on a
    /// right-hand page.
    #[test]
    fn odd_pages_are_recto() {
        assert_eq!(Side::of_number(1), Side::Recto);
        assert_eq!(Side::of_number(2), Side::Verso);
        assert_eq!(Side::of_number(3), Side::Recto);
        assert_eq!(Side::of_number(10_001), Side::Recto);
    }

    /// A chapter opens on a fresh recto page with the heading first;
    /// the verso it skips, when there is one, ships blank — so every
    /// blank page in the book is a verso.
    #[test]
    fn chapters_open_on_recto() {
        let pages = paginate(vec![
            section(long_prose(12)),
            section(vec![
                heading("Chapter Two"),
                paragraph("More prose follows here."),
            ]),
        ]);
        assert!(pages.len() > 2, "expected multi-page output");
        let mut chapter_two = None;
        for (i, page) in pages.iter().enumerate() {
            assert_eq!(page.number, i as u32 + 1);
            assert_eq!(page.side, Side::of_number(page.number));
            if page.items.is_empty() {
                assert_eq!(
                    page.side,
                    Side::Verso,
                    "page {} is a blank recto",
                    page.number
                );
            }
            if chapter_two.is_none()
                && let Some(DrawItem::Text { size, .. }) = page.items.first()
                && *size == ParagraphStyle::CHAPTER.size
            {
                chapter_two = Some(i);
            }
        }
        let index = chapter_two.expect("a page opens with the chapter heading");
        assert_eq!(pages[index].number % 2, 1, "chapter opened on a verso");
        // Fresh: the heading is the first thing painted on the page.
        assert!(
            matches!(pages[index].items.first(), Some(DrawItem::Text { size, .. }) if *size == ParagraphStyle::CHAPTER.size)
        );
    }

    /// No line crosses a page boundary: every text item's baseline
    /// sits inside its page's content box, on every page of the
    /// fixture-scale output.
    #[test]
    fn no_line_crosses_a_page_boundary() {
        let pages = paginate(vec![section(long_prose(30))]);
        assert!(pages.len() >= 3);
        let geometry = PageGeometry::trade_paperback();
        for page in &pages {
            let (x, y) = geometry.content_origin(page.side);
            let (w, h) = geometry.content_size();
            for item in &page.items {
                let DrawItem::Text {
                    x: tx,
                    y: ty,
                    glyphs,
                    ..
                } = item
                else {
                    continue;
                };
                assert!(
                    *ty >= y && *ty <= y + h,
                    "page {}: baseline {ty} outside content box",
                    page.number
                );
                assert!(
                    *tx >= x && *tx <= x + w,
                    "page {}: x {tx} outside content box",
                    page.number
                );
                for glyph in glyphs {
                    assert!(
                        glyph.x >= x - 0.5,
                        "page {}: glyph left of the box",
                        page.number
                    );
                }
            }
        }
    }

    /// Overflow starts a new page: the first baseline of every page
    /// after the first sits at the content-box top plus the strut —
    /// layout resumes there, not where the last page stopped.
    #[test]
    fn overflow_starts_a_new_page_at_the_top() {
        let pages = paginate(vec![section(long_prose(20))]);
        assert!(pages.len() >= 2);
        let geometry = PageGeometry::trade_paperback();
        for page in pages.iter().skip(1) {
            let (_, top) = geometry.content_origin(page.side);
            let first = page
                .items
                .iter()
                .find_map(|i| match i {
                    DrawItem::Text { y, .. } => Some(*y),
                    _ => None,
                })
                .expect("page has text");
            let strut = registry()
                .metrics(0)
                .map(|m| {
                    crate::linebox::Strut::from_metrics(
                        m,
                        ParagraphStyle::BODY.size,
                        ParagraphStyle::BODY.line_height,
                    )
                })
                .unwrap();
            assert!(
                (first - (top + strut.above)).abs() < 1e-3,
                "page {}: first baseline {first}, expected {}",
                page.number,
                top + strut.above
            );
        }
    }

    /// Lines stack: within a page, baselines strictly increase.
    #[test]
    fn lines_stack_down_the_page() {
        let pages = paginate(vec![section(long_prose(6))]);
        let baselines: Vec<f32> = pages[0]
            .items
            .iter()
            .filter_map(|i| match i {
                DrawItem::Text { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert!(baselines.len() > 3);
        assert!(baselines.windows(2).all(|w| w[1] > w[0]));
    }

    /// Glyph positions accumulate advances: the first glyph paints at
    /// the item origin and each subsequent glyph sits one shaped
    /// advance past its predecessor, in points.
    #[test]
    fn glyphs_are_placed_at_their_advances() {
        let pages = paginate(vec![section(vec![paragraph("hello")])]);
        let DrawItem::Text {
            x, glyphs, size, ..
        } = &pages[0].items[0]
        else {
            panic!("expected text");
        };
        assert_eq!(glyphs.len(), 5);
        assert!((glyphs[0].x - *x).abs() < 1e-4);
        let shaped = registry().shape(0, "hello").unwrap();
        let upem = registry().metrics(0).unwrap().units_per_em as f32;
        let mut expected_x = *x;
        for (glyph, shaped_glyph) in glyphs.iter().zip(&shaped) {
            assert!(
                (glyph.x - expected_x).abs() < 1e-3,
                "glyph at {}, expected {expected_x}",
                glyph.x
            );
            expected_x += shaped_glyph.x_advance as f32 / upem * size;
        }
    }

    /// A book with no content produces no pages.
    #[test]
    fn empty_book_yields_no_pages() {
        assert!(paginate(vec![]).is_empty());
        assert!(paginate(vec![section(vec![])]).is_empty());
    }

    /// The fixture book paginates.
    #[test]
    fn fixture_book_paginates() {
        let book: Book = serde_json::from_str(include_str!("../../../fixtures/book.json")).unwrap();
        let output = layout_book(&book, registry());
        assert!(!output.pages.is_empty());
        assert_eq!(output.fonts.len(), 1);
    }
}
