//! Layout: box construction, inline layout, fragmentation.
//!
//! ```text
//! content + style ─► box tree ─► line layout ─► fragmentation ─► pages
//! ```
//!
//! v0.2 folds the middle of the pipeline into one pass: each section
//! becomes its lines (each block with the style the tree computed for
//! it, via `lines::LineLayout`), and the paginator flows those lines
//! into page content boxes. Nothing here decides what anything looks
//! like — the style tree was told, and this asks it.

use crate::LayoutOutput;
use crate::content::{Block, Book, Section};
use crate::fonts::FontRegistry;
use crate::lines::{Line, LineBreakOptions, LineLayout, ParagraphStyle, ShapedRun};
use crate::pages::{DrawItem, Glyph, Page, Side};
use crate::style::{
    Align, Band, Break, Content, Hyphens, MarginBox, MarginBoxStyle, PageQuery, PageStyle,
    Situation, StyleTree,
};

/// One book through the whole pipeline: lines laid out, flowed into
/// pages, everything the output needs assembled.
pub fn layout_book(book: &Book, styles: &StyleTree, registry: &FontRegistry) -> LayoutOutput {
    let paginator = Paginator::new(registry, styles);
    LayoutOutput {
        pages: paginator.paginate(book),
        fonts: (0..registry.len() as u16)
            .filter_map(|id| registry.font_ref(id).cloned())
            .collect(),
        warnings: styles.warnings().to_vec(),
    }
}

/// What one page needs to know to ask the style tree for its master:
/// the named page in force, and the situation the page is in.
#[derive(Debug, Clone)]
struct PageSlot {
    name: Option<String>,
    /// The page a section opens on: `@page :first`.
    first: bool,
    /// Inserted to square the sheet: `@page :blank`.
    blank: bool,
}

impl PageSlot {
    fn query(&self, side: Side) -> PageQuery<'_> {
        PageQuery {
            name: self.name.as_deref(),
            situation: match (self.blank, self.first) {
                (true, _) => Situation::Blank,
                (false, true) => Situation::First(side),
                (false, false) => Situation::Body(side),
            },
        }
    }
}

/// The pagination pass: laid-out lines in, `Page`s of `DrawItem`s out.
///
/// Lines stack from the top of the content box; a line that does not
/// fit starts the next page, and a line taller than a whole page
/// overflows it. A section starts a new page, and `break-before`
/// decides which side of the spread that page falls on.
pub struct Paginator<'a> {
    registry: &'a FontRegistry,
    styles: &'a StyleTree,
    lines: LineLayout<'a>,
}

impl<'a> Paginator<'a> {
    /// A paginator over one book's styling and the faces it shapes with.
    pub fn new(registry: &'a FontRegistry, styles: &'a StyleTree) -> Self {
        Paginator {
            registry,
            styles,
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
        let mut slots = Vec::new();
        for section in &book.sections {
            let lines = self.section_lines(section);
            self.flow_section(section, &lines, &mut pages, &mut slots);
        }
        self.number_and_paint(&mut pages, &slots);
        pages
    }

    /// One section's blocks as laid-out lines, in document order:
    /// everything measurement decides, and nothing pagination does.
    pub fn section_lines(&self, section: &Section) -> Vec<Line> {
        let mut lines = Vec::new();
        let measure = self.styles.default_page().geometry.measure();
        self.append_blocks(&section.blocks, measure, &mut lines);
        lines
    }

    /// Laid-out lines in, numbered pages out: fragmentation and page
    /// assembly, one `Vec<Line>` per section of `book`. Nothing here
    /// measures — every line arrives with its box already decided.
    pub fn flow(&self, book: &Book, sections: &[Vec<Line>]) -> Vec<Page> {
        let mut pages = Vec::new();
        let mut slots = Vec::new();
        for (section, lines) in book.sections.iter().zip(sections) {
            self.flow_section(section, lines, &mut pages, &mut slots);
        }
        self.number_and_paint(&mut pages, &slots);
        pages
    }

    /// Settles numbering and side once the whole flow is assembled,
    /// then paints each page's margin boxes: a folio's digits are not
    /// known until the pages before it are.
    fn number_and_paint(&self, pages: &mut [Page], slots: &[PageSlot]) {
        for ((index, page), slot) in pages.iter_mut().enumerate().zip(slots) {
            page.number = index as u32 + 1;
            page.side = Side::of_number(page.number);
            let master = self.styles.page(slot.query(page.side));
            for which in MarginBox::ALL {
                let Some(box_style) = master.margin_box(which) else {
                    continue;
                };
                let Some((band, align)) = which.band() else {
                    continue;
                };
                self.paint_margin_box(page, master, box_style, band, align);
            }
        }
    }

    /// Paints one page margin box. Its content is a line like any
    /// other — shaped, measured, placed on the band's baseline — so
    /// furniture and prose paint through the same path.
    fn paint_margin_box(
        &self,
        page: &mut Page,
        master: &PageStyle,
        box_style: &MarginBoxStyle,
        band: Band,
        align: Align,
    ) {
        let text = match &box_style.content {
            Content::None => return,
            Content::PageNumber => page.number.to_string(),
            Content::Text(text) => text.clone(),
        };
        let style = box_style.style.paragraph();
        let Some(shaped) = self.registry.shape(style.font_id, &text) else {
            return;
        };
        let run = ShapedRun {
            font_id: style.font_id,
            size: style.size,
            text,
            text_start: 0,
            advance: shaped.iter().map(|g| g.x_advance).sum(),
            glyphs: shaped,
        };
        let line = Line {
            width: run.advance,
            box_: self.lines.line_box(std::slice::from_ref(&run), style),
            runs: vec![run],
        };
        let (band_top, _) = margin_band(master, band, style);
        let baseline = band_top + line.box_.baseline;
        let upem = self
            .registry
            .metrics(style.font_id)
            .map(|m| m.units_per_em as f32)
            .unwrap_or(1000.0);
        let text_width = line.width as f32 / upem * style.size;
        let x = match align {
            // Centred on the trim, not on the content box: a folio
            // belongs on the page's axis, and mirrored margins put
            // the content box off it.
            Align::Center => (master.geometry.width - text_width) / 2.0,
            Align::Start => master.geometry.margin.left,
            Align::End => master.geometry.width - master.geometry.margin.right - text_width,
        };
        page.items.append(&mut self.text_items(&line, x, baseline));
    }

    /// Flows one section's lines, opening a page of its own.
    fn flow_section(
        &self,
        section: &Section,
        flow: &[Line],
        pages: &mut Vec<Page>,
        slots: &mut Vec<PageSlot>,
    ) {
        if flow.is_empty() {
            return;
        }
        let style = self.styles.style(section.id);
        let name = style.page.clone();
        // `break-before: recto` on a section that would open on a
        // verso: the leaf it skips ships blank, and counts.
        if let Break::Side(wanted) = style.break_before
            && Side::of_number(pages.len() as u32 + 1) != wanted
        {
            let blank = PageSlot {
                name: None,
                first: false,
                blank: true,
            };
            pages.push(self.blank_page(&blank));
            slots.push(blank);
        }

        let mut slot = PageSlot {
            name,
            first: true,
            blank: false,
        };
        let mut master = self.master(pages.len(), &slot);
        let mut content_height = master.geometry.content_size().1;
        let mut cursor = 0f32;
        let mut items = Vec::new();

        for line in flow {
            if !items.is_empty() && cursor + line.box_.height > content_height {
                self.push_page(pages, slots, std::mem::take(&mut items), &slot);
                slot.first = false;
                master = self.master(pages.len(), &slot);
                content_height = master.geometry.content_size().1;
                cursor = 0.0;
            }
            let (x, y) = master.geometry.content_origin();
            let baseline = y + cursor + line.box_.baseline;
            items.append(&mut self.text_items(line, x, baseline));
            cursor += line.box_.height;
        }
        if !items.is_empty() {
            self.push_page(pages, slots, items, &slot);
        }
    }

    /// The master of the page that will sit at `index`.
    fn master(&self, index: usize, slot: &PageSlot) -> &PageStyle {
        self.styles
            .page(slot.query(Side::of_number(index as u32 + 1)))
    }

    /// Appends the blocks' lines, each block styled as the tree says
    /// and descending into nothing v0.2 does not yet lay out.
    fn append_blocks(&self, blocks: &[Block], measure: f32, flow: &mut Vec<Line>) {
        for block in blocks {
            match block {
                Block::Heading { id, inlines, .. } | Block::Paragraph { id, inlines, .. } => {
                    let computed = self.styles.style(*id);
                    let options = LineBreakOptions {
                        hyphenate: computed.hyphens == Hyphens::Auto,
                    };
                    flow.extend(self.lines.layout_styled(
                        inlines,
                        computed.paragraph(),
                        self.styles,
                        measure,
                        options,
                    ));
                }
                Block::Blockquote { .. } | Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }

    fn push_page(
        &self,
        pages: &mut Vec<Page>,
        slots: &mut Vec<PageSlot>,
        items: Vec<DrawItem>,
        slot: &PageSlot,
    ) {
        let mut page = self.blank_page(slot);
        page.side = Side::of_number(pages.len() as u32 + 1);
        page.items = items;
        pages.push(page);
        slots.push(slot.clone());
    }

    /// A page of the master's trim size with nothing on it. Numbering
    /// and side are settled once the whole flow is assembled.
    fn blank_page(&self, slot: &PageSlot) -> Page {
        let geometry = self.styles.page(slot.query(Side::Verso)).geometry;
        Page {
            number: 0,
            side: Side::Verso,
            width: geometry.width,
            height: geometry.height,
            items: Vec::new(),
        }
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

/// The band one margin box's line sits in: `(top, height)` in page
/// coordinates, one line tall, centred in the margin it lives in.
pub fn margin_band(master: &PageStyle, band: Band, style: ParagraphStyle) -> (f32, f32) {
    let (start, margin) = match band {
        Band::Top => (0.0, master.geometry.margin.top),
        Band::Bottom => (
            master.geometry.height - master.geometry.margin.bottom,
            master.geometry.margin.bottom,
        ),
    };
    let height = style.size * style.line_height;
    (start + margin / 2.0 - height / 2.0, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{HeadingLevel, Inline, NodeId, SourcePos};
    use crate::style::StyleTree;

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

    /// A book with its ids assigned: styling is keyed by node id, so
    /// a tree that never had ids has no styles to look up.
    fn book_of(sections: Vec<Section>) -> Book {
        let mut book = Book {
            metadata: Default::default(),
            sections,
        };
        book.assign_node_ids();
        book
    }

    fn paginate(sections: Vec<Section>) -> Vec<Page> {
        let book = book_of(sections);
        let styles = crate::style::defaults(&book, registry());
        Paginator::new(registry(), &styles).paginate(&book)
    }

    /// The built-in sheet's own answers, read back the way these
    /// tests identify what they are looking at. Nothing here is a
    /// constant: the sheet is asked.
    fn ua() -> &'static StyleTree {
        static STYLES: std::sync::OnceLock<StyleTree> = std::sync::OnceLock::new();
        STYLES.get_or_init(|| {
            let book = book_of(vec![section(vec![heading("H"), paragraph("prose")])]);
            crate::style::defaults(&book, registry())
        })
    }

    /// The font size the built-in sheet computes for one element.
    fn size_of(element: &str) -> f32 {
        let styles = ua();
        let node = styles
            .nodes()
            .iter()
            .find(|node| node.element == element)
            .unwrap_or_else(|| panic!("no {element} in the sample book"));
        styles.styles()[node.style as usize].font_size
    }

    fn body_size() -> f32 {
        size_of("p")
    }

    fn chapter_size() -> f32 {
        size_of("h1")
    }

    fn folio_size() -> f32 {
        ua().default_page()
            .margin_box(MarginBox::BottomCenter)
            .expect("the default page carries a folio")
            .style
            .font_size
    }

    /// The master a page of the fixture books resolves to.
    fn master(situation: Situation) -> &'static PageStyle {
        ua().page(PageQuery {
            name: Some("chapter"),
            situation,
        })
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

    /// The built-in sheet computes a 6×9in trim with mirrored
    /// margins; the content box is what remains, and it is the same
    /// width on both sides of the spread.
    #[test]
    fn the_built_in_sheet_computes_a_trade_paperback() {
        let recto = master(Situation::Body(Side::Recto)).geometry;
        let verso = master(Situation::Body(Side::Verso)).geometry;
        assert_eq!(recto.width, 432.0);
        assert_eq!(recto.height, 648.0);
        assert_eq!(recto.content_size(), (336.0, 540.0));
        assert_eq!(verso.content_size(), recto.content_size());
        assert_eq!(recto.measure(), 336.0);
        assert_eq!(recto.content_origin(), (54.0, 54.0));
        assert_eq!(verso.content_origin(), (42.0, 54.0));
        // The spine margin is the wider one on both sides.
        assert_eq!(recto.margin.left, verso.margin.right);
        assert_eq!(body_size(), 11.0);
        assert_eq!(chapter_size(), 18.0);
        assert_eq!(folio_size(), 9.0);
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
                && *size == chapter_size()
            {
                chapter_two = Some(i);
            }
        }
        let index = chapter_two.expect("a page opens with the chapter heading");
        assert_eq!(pages[index].number % 2, 1, "chapter opened on a verso");
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
        for page in &pages {
            let geometry = master(Situation::Body(page.side)).geometry;
            let (x, y) = geometry.content_origin();
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
                if *size == folio_size() {
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
        let body = ua().root().paragraph();
        for page in pages.iter().skip(1) {
            let (_, top) = master(Situation::Body(page.side)).geometry.content_origin();
            let first = page
                .items
                .iter()
                .find_map(|i| match i {
                    DrawItem::Text { y, size, .. } if *size != folio_size() => Some(*y),
                    _ => None,
                })
                .expect("page has text");
            let strut = registry()
                .metrics(body.font_id)
                .map(|m| crate::linebox::Strut::from_metrics(m, body.size, body.line_height))
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
                DrawItem::Text { y, size, .. } if *size != folio_size() => Some(*y),
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
    fn folio(page: &Page) -> Option<(&DrawItem, String)> {
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
    fn opens_a_chapter(page: &Page) -> bool {
        matches!(page.items.first(), Some(DrawItem::Text { size, .. }) if *size == chapter_size())
    }

    /// A chapter: a heading followed by enough prose to run over.
    fn chapter(title: &str, paragraphs: usize) -> Section {
        let mut blocks = vec![heading(title)];
        blocks.extend(long_prose(paragraphs));
        section(blocks)
    }

    /// Folios are correct and sequential — every page that carries
    /// one carries its own number, and the folios read in page order
    /// with no repeats or gaps among body pages.
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

    /// The folio is suppressed on chapter opens, because
    /// `@page chapter:first` says so. A chapter's first page counts —
    /// the next folio is one past it — but shows nothing; inserted
    /// blank versos are equally blind, by `@page :blank`.
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

    /// The folio baseline sits in the bottom margin box — strictly
    /// below the content area, inside the margin band — and is
    /// centred on the trim, not on the content box (whose mirrored
    /// margins are off-centre).
    #[test]
    fn folio_baseline_sits_in_the_margin_box() {
        let pages = paginate(vec![chapter("Chapter One", 20)]);
        let mut checked = 0;
        for page in &pages {
            let master = master(Situation::Body(page.side));
            let geometry = master.geometry;
            let folio_style = master
                .margin_box(MarginBox::BottomCenter)
                .expect("body pages carry a folio")
                .style
                .paragraph();
            let (band_top, band_height) = margin_band(master, Band::Bottom, folio_style);
            let (_, content_top) = geometry.content_origin();
            let content_bottom = content_top + geometry.content_size().1;
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
                *y + geometry.margin.bottom / 4.0 < geometry.height,
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
    /// and stays empty: the built-in sheet generates no top margin
    /// box, so nothing paints above the content box.
    #[test]
    fn running_head_slot_is_reserved_and_empty() {
        let master = master(Situation::Body(Side::Recto));
        let head = margin_band(master, Band::Top, ua().root().paragraph());
        let (_, content_top) = master.geometry.content_origin();
        assert!(head.0 > 0.0);
        assert!(
            head.0 + head.1 <= content_top,
            "running head overlaps the content box"
        );
        assert!(master.margin_box(MarginBox::TopCenter).is_none());
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
        let output = layout_fixture();
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

    /// The fixture book, laid out under the built-in sheet.
    fn layout_fixture() -> LayoutOutput {
        let mut book: Book =
            serde_json::from_str(include_str!("../../../fixtures/book.json")).unwrap();
        book.assign_node_ids();
        let styles = crate::style::defaults(&book, registry());
        layout_book(&book, &styles, registry())
    }

    /// Pagination is line layout then flow, and splitting it that way
    /// changes nothing: the harness times the two halves separately,
    /// which is only worth doing while their composition is the whole.
    #[test]
    fn the_stages_compose_into_what_paginate_does() {
        let book = book_of(vec![
            section(long_prose(30)),
            section([vec![heading("Two")], long_prose(24)].concat()),
        ]);
        let styles = crate::style::defaults(&book, registry());
        let paginator = Paginator::new(registry(), &styles);

        let staged: Vec<Vec<Line>> = book
            .sections
            .iter()
            .map(|section| paginator.section_lines(section))
            .collect();
        let by_stage = paginator.flow(&book, &staged);
        let in_one = paginator.paginate(&book);

        assert!(in_one.len() > 2, "a book worth splitting");
        assert_eq!(by_stage.len(), in_one.len());
        for (staged, whole) in by_stage.iter().zip(&in_one) {
            assert_eq!(staged.number, whole.number);
            assert_eq!(staged.side, whole.side);
            assert_eq!(format!("{:?}", staged.items), format!("{:?}", whole.items));
        }
    }

    /// The fixture book paginates, and its font table is the
    /// registry's — every cut, indexed by the id a run carries.
    #[test]
    fn fixture_book_paginates() {
        let output = layout_fixture();
        assert!(!output.pages.is_empty());
        assert_eq!(output.fonts.len(), registry().len());
    }
}
