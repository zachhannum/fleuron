//! Layout: box construction, inline layout, fragmentation.
//!
//! ```text
//! content + style ─► box tree ─► line layout ─► fragmentation ─► pages
//! ```
//!
//! v0.1 folds the middle of the pipeline into one pass: each section
//! becomes its lines (headings with the chapter style, paragraphs with
//! the body style, via `lines::LineLayout`), and the paginator flows
//! those lines into page content boxes.

use crate::LayoutOutput;
use crate::content::{Block, Book, Section};
use crate::fonts::FontRegistry;
use crate::lines::{Line, LineBreakOptions, LineLayout, ParagraphStyle, ShapedRun};
use crate::pages::{DrawItem, Glyph, Page, Side};

/// Page and content-box geometry, in points.
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
    /// 6×9in (432×648pt) with the margins of a trade paperback,
    /// mirrored across the spread.
    pub fn trade_paperback() -> PageGeometry {
        PageGeometry {
            width: 432.0,
            height: 648.0,
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

    /// The folio line's box: `(top, height)` in page coordinates. One
    /// folio line tall, centered in the bottom margin — the
    /// bottom-center margin box, clear of the content area.
    pub fn folio_line_box(self) -> (f32, f32) {
        margin_band(self.height - self.bottom, self.bottom)
    }

    /// The running-head line's box: `(top, height)`, one line tall,
    /// centered in the top margin. Geometry reserved for v0.2's
    /// string-set heads; nothing paints here yet.
    pub fn running_head_line_box(self) -> (f32, f32) {
        margin_band(0.0, self.top)
    }
}

/// The band of one furniture line, centered in a margin that runs
/// from `start` for `margin` points.
fn margin_band(start: f32, margin: f32) -> (f32, f32) {
    let height = ParagraphStyle::FOLIO.size * ParagraphStyle::FOLIO.line_height;
    (start + margin / 2.0 - height / 2.0, height)
}

/// The furniture one page role paints. v0.1 has one default master
/// per role; `@page` selectors arrive with the style compiler (#7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageMaster {
    /// Continuation pages: folio painted, running-head slot reserved.
    Body,
    /// A chapter's opening page: no folio — blind, but counted.
    ChapterOpen,
    /// A blank verso inserted to square the sheet: no furniture.
    Blank,
}

impl PageMaster {
    /// False when the page counts without showing: chapter opens and
    /// inserted blanks run blind folios.
    fn shows_folio(self) -> bool {
        matches!(self, PageMaster::Body)
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
/// Lines stack from the top of the content box; a line that does not
/// fit starts the next page, and a line taller than a whole page
/// overflows it. Each section opens on a fresh recto page, with a
/// blank verso inserted when the flow sits on one.
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
    ///
    /// A section's lines are laid out, flowed, and released before the
    /// next one is measured: what a book holds at once is its pages,
    /// not every line it was ever broken into.
    pub fn paginate(&self, book: &Book) -> Vec<Page> {
        let mut pages = Vec::new();
        let mut chapter_opens: Vec<usize> = Vec::new();
        for section in &book.sections {
            let lines = self.section_lines(section);
            if let Some(open) = self.flow_section(&lines, &mut pages) {
                chapter_opens.push(open);
            }
        }
        self.number_and_paint(&mut pages, &chapter_opens);
        pages
    }

    /// One section's blocks as laid-out lines, in document order:
    /// everything measurement decides, and nothing pagination does.
    pub fn section_lines(&self, section: &Section) -> Vec<Line> {
        let mut lines = Vec::new();
        self.append_blocks(&section.blocks, &mut lines);
        lines
    }

    /// Laid-out lines in, numbered pages out: fragmentation and page
    /// assembly, one `Vec<Line>` per section. Nothing here measures —
    /// every line arrives with its box already decided.
    pub fn flow(&self, sections: &[Vec<Line>]) -> Vec<Page> {
        let mut pages = Vec::new();
        let mut chapter_opens: Vec<usize> = Vec::new();
        for lines in sections {
            if let Some(open) = self.flow_section(lines, &mut pages) {
                chapter_opens.push(open);
            }
        }
        self.number_and_paint(&mut pages, &chapter_opens);
        pages
    }

    /// Settles numbering and side once the whole flow is assembled,
    /// then paints each page's furniture: a folio's digits are not
    /// known until the pages before it are.
    fn number_and_paint(&self, pages: &mut [Page], chapter_opens: &[usize]) {
        let masters = self.assign_masters(pages, chapter_opens);
        for ((index, page), master) in pages.iter_mut().enumerate().zip(masters) {
            page.number = index as u32 + 1;
            page.side = Side::of_number(page.number);
            self.paint_furniture(page, master);
        }
    }

    /// One master per page: a page carries body furniture unless it
    /// opens a chapter or is an inserted blank (recognized by carrying
    /// no content — only inserted pages are ever empty, since a
    /// section with nothing to paint produces no pages).
    fn assign_masters(&self, pages: &[Page], chapter_opens: &[usize]) -> Vec<PageMaster> {
        let mut masters = vec![PageMaster::Body; pages.len()];
        for (page, master) in pages.iter().zip(&mut masters) {
            if page.items.is_empty() {
                *master = PageMaster::Blank;
            }
        }
        for open in chapter_opens {
            masters[*open] = PageMaster::ChapterOpen;
        }
        masters
    }

    /// Paints the furniture of one assembled page. The folio is a
    /// line like any other — shaped, measured, placed on the margin
    /// box's baseline — so furniture and content paint through the
    /// same path. The running head's slot has geometry but no content
    /// until v0.2's string-set.
    fn paint_furniture(&self, page: &mut Page, master: PageMaster) {
        if !master.shows_folio() {
            return;
        }
        let style = ParagraphStyle::FOLIO;
        let Some(shaped) = self.registry.shape(style.font_id, &page.number.to_string()) else {
            return;
        };
        let run = ShapedRun {
            font_id: style.font_id,
            size: style.size,
            text: page.number.to_string(),
            text_start: 0,
            advance: shaped.iter().map(|g| g.x_advance).sum(),
            glyphs: shaped,
        };
        let line = Line {
            width: run.advance,
            runs: vec![run],
            box_: self.lines.line_box(&[], style),
        };
        let (band_top, _) = self.geometry.folio_line_box();
        let baseline = band_top + line.box_.baseline;
        let upem = self
            .registry
            .metrics(style.font_id)
            .map(|m| m.units_per_em as f32)
            .unwrap_or(1000.0);
        let text_width = line.width as f32 / upem * style.size;
        let x = (self.geometry.width - text_width) / 2.0;
        page.items.append(&mut self.text_items(&line, x, baseline));
    }

    /// Flows one section's lines, opening on a fresh recto. Returns
    /// the index of the page the section opens on.
    fn flow_section(&self, flow: &[Line], pages: &mut Vec<Page>) -> Option<usize> {
        if flow.is_empty() {
            return None;
        }
        if pages.len() % 2 == 1 {
            pages.push(self.blank_page());
        }
        let open = pages.len();

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
            items.append(&mut self.text_items(line, x, baseline));
            cursor += line.box_.height;
        }
        if !items.is_empty() {
            self.push_page(pages, items);
        }
        Some(open)
    }

    /// Appends the blocks' lines, descending into nothing v0.1 does
    /// not yet lay out.
    fn append_blocks(&self, blocks: &[Block], flow: &mut Vec<Line>) {
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
                Block::Blockquote { .. } | Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }

    fn push_page(&self, pages: &mut Vec<Page>, items: Vec<DrawItem>) {
        let side = self.current_side(pages);
        pages.push(Page {
            side,
            ..self.blank_page()
        });
        pages.last_mut().expect("just pushed").items = items;
    }

    /// A page of the run's trim size with nothing on it. Numbering
    /// and side are settled once the whole flow is assembled.
    fn blank_page(&self) -> Page {
        Page {
            number: 0,
            side: Side::Verso,
            width: self.geometry.width,
            height: self.geometry.height,
            items: Vec::new(),
        }
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
            for (shaped, range) in run.glyphs.iter().zip(run.glyph_ranges()) {
                glyphs.push(Glyph {
                    id: shaped.id,
                    x: glyph_x,
                    range,
                });
                glyph_x += shaped.x_advance as f32 / upem * run.size;
            }
            items.push(DrawItem::Text {
                x: x_cursor,
                y: baseline,
                font_id: run.font_id,
                size: run.size,
                text: run.text.clone(),
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
        assert!(
            matches!(pages[index].items.first(), Some(DrawItem::Text { size, .. }) if *size == ParagraphStyle::CHAPTER.size)
        );
    }

    /// No line crosses a page boundary: every content baseline sits
    /// inside its page's content box, on every page of the
    /// fixture-scale output. The folio is exempt — it lives in the
    /// bottom margin box on purpose, and `folios_are_correct` proves
    /// where.
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
                    size,
                    ..
                } = item
                else {
                    continue;
                };
                if *size == ParagraphStyle::FOLIO.size {
                    continue;
                }
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
    /// layout resumes there, not where the last page stopped. The
    /// folio paints after content, so it is never the first item.
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
                    DrawItem::Text { y, size, .. } if *size != ParagraphStyle::FOLIO.size => {
                        Some(*y)
                    }
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

    /// Lines stack: within a page, content baselines strictly
    /// increase; the folio comes after the last of them.
    #[test]
    fn lines_stack_down_the_page() {
        let pages = paginate(vec![section(long_prose(6))]);
        let baselines: Vec<f32> = pages[0]
            .items
            .iter()
            .filter_map(|i| match i {
                DrawItem::Text { y, size, .. } if *size != ParagraphStyle::FOLIO.size => Some(*y),
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

    /// Total advance of a folio run, in points — for checking where
    /// the centered run sits on the trim.
    fn run_width_pt(glyphs: &[Glyph], size: f32) -> f32 {
        let font = ParagraphStyle::FOLIO.font_id;
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
    fn folio(page: &Page) -> Option<(&DrawItem, String)> {
        page.items.iter().find_map(|item| match item {
            DrawItem::Text {
                size,
                font_id,
                glyphs,
                ..
            } if *size == ParagraphStyle::FOLIO.size => {
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
    fn opens_a_chapter(page: &Page) -> bool {
        matches!(page.items.first(), Some(DrawItem::Text { size, .. }) if *size == ParagraphStyle::CHAPTER.size)
    }

    /// A chapter: a heading followed by enough prose to run over.
    fn chapter(title: &str, paragraphs: usize) -> Section {
        let mut blocks = vec![heading(title)];
        blocks.extend(long_prose(paragraphs));
        section(blocks)
    }

    /// Acceptance: folios are correct and sequential — every page
    /// that carries one carries its own number, and the folios read
    /// in page order with no repeats or gaps among body pages.
    #[test]
    fn folios_are_correct_and_sequential() {
        let pages = paginate(vec![chapter("Chapter One", 24), chapter("Chapter Two", 24)]);
        assert!(
            pages.len() > 4,
            "expected a multi-page book, got {}",
            pages.len()
        );
        let mut numbered = Vec::new();
        for page in &pages {
            if let Some((_, digits)) = folio(page) {
                assert_eq!(
                    digits,
                    page.number.to_string(),
                    "page {} shows folio {digits}",
                    page.number
                );
                numbered.push(page.number);
            }
        }
        assert!(numbered.len() >= 2, "expected folios on the body pages");
        assert!(
            numbered.windows(2).all(|w| w[1] > w[0]),
            "folios out of order: {numbered:?}"
        );
    }

    /// Acceptance: the folio is suppressed on chapter opens. A
    /// chapter's first page counts — the next folio is one past it —
    /// but shows nothing; inserted blank versos are equally blind.
    #[test]
    fn folios_are_suppressed_on_chapter_opens() {
        let pages = paginate(vec![chapter("Chapter One", 14), chapter("Chapter Two", 14)]);
        let opens: Vec<u32> = pages
            .iter()
            .filter(|p| opens_a_chapter(p))
            .map(|p| p.number)
            .collect();
        assert_eq!(opens.len(), 2, "two chapters, two opening pages");
        for page in &pages {
            let blind = opens_a_chapter(page) || page.items.is_empty();
            assert_eq!(
                folio(page).is_some(),
                !blind,
                "page {}: folio presence wrong (opens chapter: {}, blank: {})",
                page.number,
                opens_a_chapter(page),
                page.items.is_empty()
            );
        }
        // Counted, not shown: the page after an open carries its own
        // number, one past the blind one.
        for open in opens {
            if let Some(next) = pages.get(open as usize) {
                assert_eq!(folio(next).map(|(_, d)| d), Some((open + 1).to_string()));
            }
        }
    }

    /// Acceptance: the folio baseline sits in the bottom margin box —
    /// strictly below the content area, inside the margin band — and
    /// the folio is centered on the trim, not on the content box
    /// (whose mirrored margins are off-center).
    #[test]
    fn folio_baseline_sits_in_the_margin_box() {
        let pages = paginate(vec![chapter("Chapter One", 20)]);
        let geometry = PageGeometry::trade_paperback();
        let (band_top, band_height) = geometry.folio_line_box();
        let (_, content_top) = geometry.content_origin(Side::Recto);
        let content_bottom = content_top + geometry.content_size().1;
        let mut checked = 0;
        for page in &pages {
            let Some((
                DrawItem::Text {
                    x, y, glyphs, size, ..
                },
                _,
            )) = folio(page)
            else {
                continue;
            };
            assert!(
                *y > content_bottom,
                "page {}: folio baseline {y} is inside the content area (bottom {content_bottom})",
                page.number
            );
            assert!(
                *y >= band_top && *y <= band_top + band_height,
                "page {}: folio baseline {y} outside the margin box [{band_top}, {}]",
                page.number,
                band_top + band_height
            );
            assert!(
                *y + geometry.bottom / 4.0 < geometry.height,
                "page {}: folio baseline {y} runs off the trim",
                page.number
            );
            let width = run_width_pt(glyphs, *size);
            assert!(
                (x + width / 2.0 - geometry.width / 2.0).abs() < 1e-3,
                "page {}: folio centered at {}, trim center {}",
                page.number,
                x + width / 2.0,
                geometry.width / 2.0
            );
            checked += 1;
        }
        assert!(checked >= 2, "expected folios to check");
    }

    /// The running-head slot is reserved geometry in the top margin
    /// and stays empty: nothing paints above the content box.
    #[test]
    fn running_head_slot_is_reserved_and_empty() {
        let geometry = PageGeometry::trade_paperback();
        let (head_top, head_height) = geometry.running_head_line_box();
        let (_, content_top) = geometry.content_origin(Side::Recto);
        assert!(head_top > 0.0);
        assert!(
            head_top + head_height <= content_top,
            "running head overlaps the content box"
        );
        for page in paginate(vec![chapter("Chapter One", 14)]) {
            for item in &page.items {
                if let DrawItem::Text { y, .. } = item {
                    assert!(
                        *y >= content_top,
                        "page {}: something painted in the running-head slot at {y}",
                        page.number
                    );
                }
            }
        }
    }

    /// A book with no content produces no pages.
    #[test]
    fn empty_book_yields_no_pages() {
        assert!(paginate(vec![]).is_empty());
        assert!(paginate(vec![section(vec![])]).is_empty());
    }

    /// The fixture book carries folios: the e2e path exercises page
    /// furniture, not just content flow.
    #[test]
    fn fixture_book_carries_folios() {
        let book: Book = serde_json::from_str(include_str!("../../../fixtures/book.json")).unwrap();
        let output = layout_book(&book, registry());
        let with_folios = output.pages.iter().filter(|p| folio(p).is_some()).count();
        assert!(
            with_folios >= output.pages.len() - 2,
            "only {with_folios} of {} fixture pages carry folios",
            output.pages.len()
        );
        for page in &output.pages {
            if let Some((_, digits)) = folio(page) {
                assert_eq!(digits, page.number.to_string());
            }
        }
    }

    /// Pagination is line layout then flow, and splitting it that way
    /// changes nothing: the harness times the two halves separately,
    /// which is only worth doing while their composition is the whole.
    #[test]
    fn the_stages_compose_into_what_paginate_does() {
        let book = Book {
            metadata: Default::default(),
            sections: vec![
                section(long_prose(30)),
                section([vec![heading("Two")], long_prose(24)].concat()),
            ],
        };
        let paginator = Paginator::new(registry(), PageGeometry::trade_paperback());

        let staged: Vec<Vec<Line>> = book
            .sections
            .iter()
            .map(|section| paginator.section_lines(section))
            .collect();
        let by_stage = paginator.flow(&staged);
        let in_one = paginator.paginate(&book);

        assert!(in_one.len() > 2, "a book worth splitting");
        assert_eq!(by_stage.len(), in_one.len());
        for (staged, whole) in by_stage.iter().zip(&in_one) {
            assert_eq!(staged.number, whole.number);
            assert_eq!(staged.side, whole.side);
            assert_eq!(format!("{:?}", staged.items), format!("{:?}", whole.items));
        }
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
