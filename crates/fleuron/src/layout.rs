//! Layout: box construction, inline layout, fragmentation.
//!
//! ```text
//! content + style ─► box tree ─► line layout ─► fragmentation ─► pages
//! ```
//!
//! v0.2 folds the middle of the pipeline into one pass: each section
//! becomes fragments (each block with the style the tree computed for
//! it, via `lines::LineLayout`), and the paginator flows those
//! fragments into page content boxes. Nothing here decides what
//! anything looks like — the style tree was told, and this asks it.
//!
//! A fragment is what the flow can move: one line, one image, one
//! ornament. Where a page may end is decided when the fragments are
//! built — orphans, widows, `break-inside`, an ornament that must
//! keep the prose around it — and the flow only stacks and, when
//! something does not fit, walks back to the last place a break was
//! allowed.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::content::{Block, Book, Inline, Metadata, NodeId, Section, SourceRange, origin, text};
use crate::fonts::FontRegistry;
use crate::images::Assets;
use crate::lines::{
    Line, LineBreakOptions, LineLayout, Measure, Opening, ParagraphStyle, Patterns, Span,
};
use crate::pages::{DrawItem, Glyph, Page, Side};
use crate::session::Session;
use crate::style::{
    Align, Band, BoxDecorationBreak, Break, Color, ComputedStyle, Content, Edges, Hyphens,
    MarginBox, MarginBoxStyle, PageQuery, PageStyle, Situation, StringPiece, StyleTree, TextAlign,
    TextJustify,
};
use crate::{LayoutOutput, Warning};

/// One book through the whole pipeline: lines laid out, flowed into
/// pages, everything the output needs assembled.
///
/// A single run over a session that retains nothing. It keeps one
/// section's lines at a time, which is what a process that renders a
/// book once and exits wants. A live preview uses `Session` instead.
///
/// `assets` is the images the host probed. A book with none of them
/// passes [`Assets::none`].
pub fn layout_book(
    book: &Book,
    styles: &StyleTree,
    registry: &FontRegistry,
    assets: &Assets,
) -> LayoutOutput {
    Session::once(book, styles, registry, assets).into_output()
}

/// The asset table of a host that supplied none.
pub(crate) fn no_assets() -> &'static Assets {
    static EMPTY: std::sync::OnceLock<Assets> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Assets::none)
}

/// The fonts a run used, in the order the output indexes them.
pub(crate) fn font_table(registry: &FontRegistry) -> Vec<crate::fonts::FontRefEntry> {
    (0..registry.len() as u16)
        .filter_map(|id| registry.font_ref(id).cloned())
        .collect()
}

/// Whether a page may end above a fragment.
///
/// Everything the cascade says about fragmentation — `break-before`,
/// `break-after`, `break-inside`, `orphans`, `widows` — reaches the
/// flow as one of these, decided while the fragments are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakPoint {
    /// A page break must fall here, on the side the break names.
    Forced(Break),
    /// A break may fall here.
    Allowed,
    /// A break may not fall here: this fragment moves with the one
    /// above it.
    Forbidden,
}

/// What one fragment paints.
#[derive(Debug, Clone)]
pub enum Piece {
    /// A laid-out line, and the initial letter sunk beside it.
    Line {
        /// The line itself, shaped and measured.
        line: Line,
        /// The drop cap set beside this line, on the first line of a
        /// paragraph that has one.
        cap: Option<DropCap>,
    },
    /// A placed image, sized against the content box.
    Image {
        /// Width in points, after any scaling.
        width: f32,
        /// Height in points, after any scaling.
        height: f32,
        /// Index into the asset table.
        asset: u32,
    },
    /// Space with nothing in it: what a thematic break set in space
    /// rather than in an ornament comes to.
    Blank,
}

/// An initial letter sunk beside the lines that follow it.
#[derive(Debug, Clone)]
pub struct DropCap {
    /// The letter, shaped at the size the sink works out to.
    pub line: Line,
    /// Leading edge, from the content box's own.
    pub x: f32,
    /// How far the cap's baseline sits below its line's.
    pub drop: f32,
}

/// One thing the flow can place: a line, an image, an ornament.
///
/// Everything horizontal is settled here — indentation, alignment,
/// the measure a drop cap left — so the flow only stacks.
#[derive(Debug, Clone)]
pub struct Fragment {
    /// Leading edge, from the content box's own.
    pub x: f32,
    /// Space above, from the margins around it. A page that opens on
    /// this fragment drops it.
    pub lead: f32,
    /// Space above that a page break does not drop and no margin
    /// collapses through: the borders and padding between this
    /// fragment and whatever is above it.
    pub fixed: f32,
    /// The fragment's own height.
    pub height: f32,
    /// Whether a page may end above it.
    pub break_before: BreakPoint,
    /// What it paints.
    pub piece: Piece,
    /// What it tells the page furniture when it lands. Boxed because
    /// a book has thousands of fragments and a handful of chapter
    /// headings.
    pub marks: Option<Box<Marks>>,
    /// The decorated blocks this fragment opens and closes. Boxed
    /// for the same reason: most fragments decorate nothing.
    pub decorations: Option<Box<Decorations>>,
}

/// What one fragment does to the blocks decorated around it.
///
/// A decoration spans a range of fragments. The range is settled
/// while the flow is built and the geometry is not: the paginator
/// places fragments one at a time and moves what it has already
/// painted when one carries to the next page. So the range travels on
/// the fragments at its ends, and the paginator resolves it, per
/// page, into a border box over the fragments that landed there.
#[derive(Debug, Clone, Default)]
pub struct Decorations {
    /// The blocks whose first fragment this is, outermost first.
    pub opens: Vec<Decoration>,
    /// How many of the open blocks end with this fragment,
    /// innermost first.
    pub closes: u32,
}

/// One decorated block: what it paints, and where it sits around the
/// fragments it spans.
#[derive(Debug, Clone)]
pub struct Decoration {
    /// Leading edge of the border box, from the content box's own.
    pub x: f32,
    /// Width of the border box.
    pub width: f32,
    /// Distance from the top of the block's first fragment up to the
    /// top of its border box.
    pub above: f32,
    /// Distance from the bottom of its last fragment down to the
    /// bottom of its border box.
    pub below: f32,
    /// Border widths, zero on an edge that is not drawn.
    pub border: Edges,
    /// What each edge is painted in, `currentColor` resolved.
    pub colors: Edges<Color>,
    /// What is painted behind the whole border box.
    pub background: Option<Color>,
    /// Whether `box-decoration-break: clone` closes the two edges a
    /// page break cuts.
    pub cloned: bool,
}

/// What a fragment tells the page it lands on: the running strings
/// its element set, and the folio its page restarts at.
///
/// Both are captured from the content flow, so both are answers only
/// pagination has: which page a heading fell on is not known until it
/// falls there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marks {
    /// Named strings, resolved from the element's own text, in the
    /// order the cascade gave them.
    pub strings: Vec<(String, String)>,
    /// The folio of the page this fragment lands on.
    pub page_number: Option<u32>,
}

/// The running strings in force, by name.
type Strings = BTreeMap<String, String>;

/// What one page's furniture resolves its content against: the folio
/// the page counted to, and the running strings it opened with.
struct Furniture<'a> {
    folio: u32,
    strings: &'a Strings,
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

/// The pagination pass: content in, `Page`s of `DrawItem`s out.
///
/// Fragments stack from the top of the content box. One that does not
/// fit ends the page — at the last point a break was allowed, which
/// may be several fragments back — and a fragment taller than a whole
/// page overflows it.
pub struct Paginator<'a> {
    registry: &'a FontRegistry,
    styles: &'a StyleTree,
    assets: &'a Assets,
    lines: LineLayout<'a>,
    /// The syllable patterns `hyphens: auto` breaks by.
    patterns: Cell<Patterns>,
    /// The declared language there are no patterns for, which the
    /// first paragraph that asks to be hyphenated complains about.
    unknown: RefCell<Option<String>>,
    /// What fragmentation had to complain about. Recorded once per
    /// message: a book that scales the same image twice has one
    /// problem, not two.
    warnings: RefCell<Vec<Warning>>,
}

impl<'a> Paginator<'a> {
    /// A paginator over one book's styling and the faces it shapes
    /// with, for a book with no images in it.
    pub fn new(registry: &'a FontRegistry, styles: &'a StyleTree) -> Self {
        Paginator::with_assets(registry, styles, no_assets())
    }

    /// The same, over images the host has already probed.
    pub fn with_assets(
        registry: &'a FontRegistry,
        styles: &'a StyleTree,
        assets: &'a Assets,
    ) -> Self {
        Paginator {
            registry,
            styles,
            assets,
            lines: LineLayout::new(registry),
            patterns: Cell::new(Patterns::default()),
            unknown: RefCell::new(None),
            warnings: RefCell::new(Vec::new()),
        }
    }

    /// Takes the hyphenation patterns from the language the book
    /// declares. `paginate` reads them from the book it is handed. A
    /// caller that builds one section's fragments on its own sets
    /// them here.
    ///
    /// A language with no patterns leaves every word whole, rather
    /// than breaking one language's words at another's syllables,
    /// and the first paragraph that asks for hyphenation says so.
    pub fn language(&self, metadata: &Metadata) {
        let patterns = Patterns::of(metadata);
        self.patterns.set(patterns);
        *self.unknown.borrow_mut() = metadata
            .language()
            .filter(|_| patterns == Patterns::NONE)
            .map(str::to_string);
    }

    /// What `hyphens: auto` has to break with, complaining the first
    /// time it is asked for and there is nothing behind the language
    /// the book declares.
    fn patterns(&self) -> Patterns {
        if let Some(tag) = self.unknown.borrow().as_deref() {
            self.warn(
                format!("no hyphenation patterns for language `{tag}`; its words are left whole"),
                None,
            );
        }
        self.patterns.get()
    }

    /// What fragmentation had to complain about.
    pub fn warnings(&self) -> Vec<Warning> {
        self.warnings.borrow().clone()
    }

    /// Flows one book into numbered, side-tagged pages.
    ///
    /// A section's fragments are built, flowed, and released before
    /// the next one is measured: what exists at once is the book's
    /// pages, not every line it was ever broken into.
    pub fn paginate(&self, book: &Book) -> Vec<Page> {
        self.language(&book.metadata);
        let mut flow = Flow::new(self);
        for section in &book.sections {
            let fragments = self.section_fragments(section);
            flow.section(section, &fragments);
        }
        let mut paged = flow.finish();
        self.paint(&mut paged.pages, &paged.infos);
        paged.pages
    }

    /// One section's blocks as fragments, in document order:
    /// everything measurement decides, and nothing pagination does.
    pub fn section_fragments(&self, section: &Section) -> Vec<Fragment> {
        let measure = self.styles.default_page().geometry.measure();
        let mut builder = Builder {
            paginator: self,
            source: section.source.as_deref(),
            fragments: Vec::new(),
            pending: BreakPoint::Allowed,
            margin: 0.0,
            fixed: 0.0,
            pending_marks: None,
            open: Vec::new(),
        };
        let style = self.styles.style(section.id).clone();
        let start = builder.open(&style, &[], 0.0, measure);
        let (x, narrowed) = style.content_box(0.0, measure);
        builder.blocks(&section.blocks, x, narrowed);
        builder.close(&style, start);
        builder.fragments
    }

    /// Fragments in, numbered pages out: fragmentation and page
    /// assembly, one `Vec<Fragment>` per section of `book`. Nothing
    /// here measures — every fragment arrives with its box decided.
    pub fn flow(&self, book: &Book, sections: &[Vec<Fragment>]) -> Vec<Page> {
        let mut paged = self.fragment(book, sections.iter().map(Vec::as_slice));
        self.paint(&mut paged.pages, &paged.infos);
        paged.pages
    }

    /// The same, stopping short of the furniture: pages as the flow
    /// settled them, and what each one needs to paint its own.
    pub(crate) fn fragment<'f>(
        &self,
        book: &Book,
        sections: impl IntoIterator<Item = &'f [Fragment]>,
    ) -> Paged {
        let mut flow = Flow::new(self);
        for (section, fragments) in book.sections.iter().zip(sections) {
            flow.section(section, fragments);
        }
        flow.finish()
    }

    /// Settles numbering and side once the whole flow is assembled,
    /// then paints each page's margin boxes: a folio's digits are not
    /// known until the pages before it are.
    ///
    /// Idempotent. What an earlier paint left is discarded first, so
    /// a session that only changed its furniture repaints in place.
    ///
    /// The folio counts pages, and `counter-reset: page` restarts it
    /// where a section asked; the side counts leaves, and nothing
    /// restarts that — recto and verso are where a page falls in the
    /// sheet, not what is printed on it.
    ///
    /// A page inserted to square the sheet paints no furniture. A
    /// blank leaf is blank: a page whose only content would be a
    /// running head does not get one.
    pub(crate) fn paint(&self, pages: &mut [Page], infos: &[PageInfo]) {
        let mut folio = 0;
        for ((index, page), info) in pages.iter_mut().enumerate().zip(infos) {
            // Furniture is appended after the page's own content, so
            // dropping the tail is all a repaint has to undo.
            page.items.truncate(info.content_items);
            folio = info.reset.unwrap_or(folio + 1);
            page.number = folio;
            page.side = Side::of_number(index as u32 + 1);
            if info.slot.blank {
                continue;
            }
            let master = self.styles.page(info.slot.query(page.side));
            for which in MarginBox::ALL {
                let Some(box_style) = master.margin_box(which) else {
                    continue;
                };
                let Some((band, align)) = which.band() else {
                    continue;
                };
                let furniture = Furniture {
                    folio,
                    strings: &info.strings,
                };
                self.paint_margin_box(page, master, box_style, band, align, furniture);
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
        furniture: Furniture<'_>,
    ) {
        let text = match &box_style.content {
            Content::None => return,
            Content::Counter(counter) => counter.format(furniture.folio),
            Content::String(name) => furniture.strings.get(name).cloned().unwrap_or_default(),
            Content::Text(text) => text.clone(),
        };
        if text.is_empty() {
            return;
        }
        let style = box_style.style.paragraph();
        let Some(line) = self.line_of(&text, style) else {
            return;
        };
        let (band_top, _) = margin_band(master, band, style);
        let baseline = band_top + line.box_.baseline;
        let text_width = self.line_width(&line);
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

    /// The master of the page that will sit at `index`.
    fn master(&self, index: usize, slot: &PageSlot) -> &PageStyle {
        self.styles
            .page(slot.query(Side::of_number(index as u32 + 1)))
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
            sections: Vec::new(),
            items: Vec::new(),
        }
    }

    /// One string as a single shaped line: page furniture, and the
    /// ornaments and initial letters that are content but not prose.
    fn line_of(&self, text: &str, style: ParagraphStyle) -> Option<Line> {
        let runs = self.lines.shape(text, style)?;
        let box_ = self.lines.line_box(&runs, style);
        Some(Line::of(runs, box_))
    }

    /// Design units per em of a face, for the one conversion that
    /// takes shaped advances into points.
    fn upem(&self, font_id: u16) -> f32 {
        self.registry
            .metrics(font_id)
            .map(|m| m.units_per_em as f32)
            .unwrap_or(1000.0)
    }

    /// A line's width in points. Runs of different sizes each convert
    /// against their own face: font units do not commute across sizes.
    /// What hangs into a margin is not part of the width, which is the
    /// point of hanging it.
    fn line_width(&self, line: &Line) -> f32 {
        line.runs
            .iter()
            .map(|run| run.advance as f32 / self.upem(run.font_id) * run.size)
            .sum::<f32>()
            - line.overhang
            - line.protrusion
    }

    /// One span's width in points. Runs of different sizes each
    /// convert against their own face: font units do not commute
    /// across sizes. A band hangs into the margins at its own two
    /// ends, never at a span boundary inside it.
    fn span_width(&self, line: &Line, index: usize) -> f32 {
        let span = &line.spans[index];
        let ink = line.runs[span.runs.clone()]
            .iter()
            .map(|run| run.advance as f32 / self.upem(run.font_id) * run.size)
            .sum::<f32>();
        let overhang = if index + 1 == line.spans.len() {
            line.overhang
        } else {
            0.0
        };
        let protrusion = if index == 0 { line.protrusion } else { 0.0 };
        ink - overhang - protrusion
    }

    /// Records one diagnostic, once. A book that hits the same
    /// problem on every page has one problem.
    fn warn(&self, message: String, origin: Option<String>) {
        let mut warnings = self.warnings.borrow_mut();
        if !warnings.iter().any(|seen| seen.message == message) {
            warnings.push(Warning { message, origin });
        }
    }

    /// One line as paint ops: every run a `DrawItem::Text` at the
    /// baseline, glyphs placed at their accumulated advances, and
    /// each span of the line opened at its own origin.
    fn text_items(&self, line: &Line, x: f32, baseline: f32) -> Vec<DrawItem> {
        let mut items = Vec::new();
        for span in line.spans.iter() {
            let mut x_cursor = x + span.offset;
            for run in &line.runs[span.runs.clone()] {
                let upem = self.upem(run.font_id);
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
                    source: run.source.clone(),
                    source_map: run.source_map.clone(),
                    origin: run.origin.clone(),
                    features: run.features,
                    color: run.color,
                    glyphs,
                });
                x_cursor = glyph_x;
            }
        }
        items
    }
}

/// Whether a block paints anything behind or around its content.
fn decorated(style: &ComputedStyle) -> bool {
    style.background_color.is_some() || style.border.paints()
}

/// The decoration one block paints, or `None` where it paints
/// nothing. `x` and `measure` are what the block was laid out
/// against; the border box takes its margins off them.
fn decoration(style: &ComputedStyle, x: f32, measure: f32) -> Option<Decoration> {
    if !decorated(style) {
        return None;
    }
    let (left, width) = style.border_box(x, measure);
    let border = style.border.widths();
    let ink = |edge: crate::style::Border| edge.color.unwrap_or(style.color);
    Some(Decoration {
        x: left,
        width,
        above: 0.0,
        below: 0.0,
        border,
        colors: Edges {
            top: ink(style.border.top),
            right: ink(style.border.right),
            bottom: ink(style.border.bottom),
            left: ink(style.border.left),
        },
        background: style.background_color,
        cloned: style.box_decoration_break == BoxDecorationBreak::Clone,
    })
}

/// The initial letter of one paragraph, sized and shaped, with the
/// text it was taken out of.
struct Cap {
    /// The letter, shaped at the size the sink works out to.
    line: Line,
    /// Width the lines beside it give up, the gutter included.
    reserved: f32,
    /// Lines it is sunk over.
    lines: usize,
}

/// Builds one section's fragments: blocks in, everything the flow
/// needs to place them out.
struct Builder<'a, 'p> {
    paginator: &'p Paginator<'a>,
    /// The file the section was read from, for diagnostics.
    source: Option<&'p str>,
    fragments: Vec<Fragment>,
    /// What the cascade has asked for above the next fragment.
    pending: BreakPoint,
    /// The collapsible margin standing above the next fragment,
    /// which is the larger of the margins that met there.
    margin: f32,
    /// Space above the next fragment that no margin collapses
    /// through: the borders and padding the blocks around it set.
    fixed: f32,
    /// What the blocks opened so far have set, waiting for a fragment
    /// to attach it to a page.
    pending_marks: Option<Box<Marks>>,
    /// The decorated blocks still open, outermost first.
    open: Vec<Pending>,
}

/// A decorated block while its fragments are still being built.
struct Pending {
    /// The fragment its first one will be.
    start: usize,
    /// What `fixed` stood at when the block's border box opened, and
    /// what the distance down to its first fragment is measured from.
    open_fixed: f32,
    /// Whether anything has been committed above the block's first
    /// fragment yet. Until something has, a margin committed inside
    /// the block is one that collapsed through it, and sits above its
    /// border box rather than inside it.
    started: bool,
    decoration: Decoration,
}

impl Builder<'_, '_> {
    fn styles(&self) -> &StyleTree {
        self.paginator.styles
    }

    /// Folds one `break-before` or `break-after` into what is already
    /// asked above the next fragment. A forced break outranks an
    /// avoided one, and either outranks `auto`. A page break outranks
    /// a column break, being the same break carried further.
    fn ask(&mut self, wanted: Break) {
        self.pending = match (self.pending, wanted) {
            (BreakPoint::Forced(Break::Column), Break::Page | Break::Side(_)) => {
                BreakPoint::Forced(wanted)
            }
            (BreakPoint::Forced(forced), _) => BreakPoint::Forced(forced),
            (_, Break::Page) => BreakPoint::Forced(Break::Page),
            (_, Break::Side(side)) => BreakPoint::Forced(Break::Side(side)),
            (_, Break::Column) => BreakPoint::Forced(Break::Column),
            (_, Break::Avoid) => BreakPoint::Forbidden,
            (pending, Break::Auto) => pending,
        };
    }

    /// Opens a block: what it asks for above itself, what it sets for
    /// the page furniture, the space its top margin leaves, and the
    /// decoration it paints across `x` and `measure`.
    ///
    /// Adjacent margins collapse to the larger. A top border or
    /// padding is not a margin: it takes height of its own, and the
    /// margins on either side of it no longer meet.
    fn open(&mut self, style: &ComputedStyle, inlines: &[Inline], x: f32, measure: f32) -> usize {
        self.ask(style.break_before);
        self.mark(style, inlines);
        self.margin = self.margin.max(style.margin.top);
        let start = self.fragments.len();
        let border = style.border.widths();
        if let Some(decoration) = decoration(style, x, measure) {
            self.open.push(Pending {
                start,
                open_fixed: self.fixed,
                started: false,
                decoration,
            });
        }
        if border.top + style.padding.top > 0.0 {
            let margin = std::mem::take(&mut self.margin);
            self.commit(margin);
            self.fixed += border.top + style.padding.top;
        }
        start
    }

    /// Commits space no margin collapses through. The margin a
    /// border or padding shuts off is outside every block that has
    /// not opened a box of its own yet, and inside every block that
    /// has.
    fn commit(&mut self, amount: f32) {
        for pending in self.open.iter_mut().rev() {
            if pending.started {
                break;
            }
            pending.started = true;
            pending.open_fixed += amount;
        }
        self.fixed += amount;
    }

    /// Resolves what one element sets — its `string-set` values, the
    /// folio its page takes — against its own text. It lands on the
    /// first fragment the element emits, which is the fragment whose
    /// page the element is on.
    ///
    /// An element that emits nothing hands what it set to whatever
    /// comes next: a string set by an empty heading is still set.
    fn mark(&mut self, style: &ComputedStyle, inlines: &[Inline]) {
        if style.string_set.is_empty() && style.counter_reset.is_none() {
            return;
        }
        let mut cached = None;
        let marks = self.pending_marks.get_or_insert_with(Box::default);
        for set in &style.string_set {
            let mut value = String::new();
            for piece in &set.value {
                match piece {
                    StringPiece::Content => {
                        value.push_str(cached.get_or_insert_with(|| text(inlines)))
                    }
                    StringPiece::Text(literal) => value.push_str(literal),
                }
            }
            marks.strings.push((set.name.clone(), value));
        }
        if let Some(folio) = style.counter_reset {
            marks.page_number = Some(folio);
        }
    }

    /// Closes a block: `break-inside: avoid` glues everything it
    /// emitted, its bottom border and padding take height of their
    /// own, and its bottom margin becomes the next block's lead.
    fn close(&mut self, style: &ComputedStyle, start: usize) {
        if style.break_inside == Break::Avoid {
            for fragment in self.fragments.iter_mut().skip(start + 1) {
                fragment.break_before = BreakPoint::Forbidden;
            }
        }
        let border = style.border.widths();
        if border.bottom + style.padding.bottom > 0.0 {
            let margin = std::mem::take(&mut self.margin);
            self.commit(margin);
            self.fixed += border.bottom + style.padding.bottom;
        }
        if decorated(style) {
            let pending = self.open.pop().expect("the block opened a decoration");
            self.seal(pending);
        }
        self.margin = self.margin.max(style.margin.bottom);
        // A block that emitted nothing settles nothing: what was
        // asked above it is still asked above whatever comes next.
        if self.fragments.len() > start {
            self.pending = BreakPoint::Allowed;
        }
        self.ask(style.break_after);
    }

    /// Hands one block's decoration to the fragments at the ends of
    /// its range, which is where the paginator reads it back. A block
    /// that emitted nothing has no range and paints nothing.
    fn seal(&mut self, pending: Pending) {
        let end = self.fragments.len();
        if end == pending.start {
            return;
        }
        let mut decoration = pending.decoration;
        // Everything committed since the last fragment was emitted
        // lies inside the block that fragment was in.
        decoration.below = self.fixed;
        // Outermost first, so the paginator's stack pops the
        // innermost block that ends at a fragment.
        Builder::decorations(&mut self.fragments[pending.start])
            .opens
            .insert(0, decoration);
        Builder::decorations(&mut self.fragments[end - 1]).closes += 1;
    }

    fn decorations(fragment: &mut Fragment) -> &mut Decorations {
        fragment.decorations.get_or_insert_with(Box::default)
    }

    /// Emits the one fragment a block is: everything the cascade asked
    /// for above the block goes on it, there being no other fragment
    /// for it.
    fn emit_one(&mut self, x: f32, height: f32, piece: Piece) {
        self.emit(&mut true, BreakPoint::Allowed, x, height, piece);
    }

    /// Emits one fragment. The first of a block gets the break the
    /// cascade asked for above it and the space its margins left; the
    /// rest get what the block says about splitting itself.
    fn emit(&mut self, first: &mut bool, inner: BreakPoint, x: f32, height: f32, piece: Piece) {
        // This fragment settles how far the border box of every
        // block opening on it sits above it.
        let (index, fixed) = (self.fragments.len(), self.fixed);
        for pending in &mut self.open {
            if pending.start == index {
                pending.decoration.above = fixed - pending.open_fixed;
            }
        }
        let (break_before, lead, fixed, marks) = if *first {
            *first = false;
            (
                std::mem::replace(&mut self.pending, BreakPoint::Allowed),
                std::mem::take(&mut self.margin),
                std::mem::take(&mut self.fixed),
                self.pending_marks.take(),
            )
        } else {
            (inner, 0.0, 0.0, None)
        };
        self.fragments.push(Fragment {
            x,
            lead,
            fixed,
            height,
            break_before,
            piece,
            marks,
            decorations: None,
        });
    }

    /// Every block of one nesting level, at `x` from the content
    /// box's leading edge and breaking to `measure`.
    fn blocks(&mut self, blocks: &[Block], x: f32, measure: f32) {
        for block in blocks {
            match block {
                Block::Heading { id, inlines, .. } | Block::Paragraph { id, inlines, .. } => {
                    self.paragraph(*id, inlines, x, measure);
                }
                Block::Blockquote { id, blocks, .. } => {
                    let style = self.styles().style(*id).clone();
                    let start = self.open(&style, &[], x, measure);
                    let (inner, narrowed) = style.content_box(x, measure);
                    self.blocks(blocks, inner, narrowed);
                    self.close(&style, start);
                }
                Block::ThematicBreak { id, .. } => {
                    let style = self.styles().style(*id).clone();
                    let start = self.open(&style, &[], x, measure);
                    self.ornament(&style, x, measure);
                    self.close(&style, start);
                }
                Block::Image {
                    id, url, position, ..
                } => {
                    let style = self.styles().style(*id).clone();
                    let start = self.open(&style, &[], x, measure);
                    self.image(&style, url, origin(self.source, *position), x, measure);
                    self.close(&style, start);
                }
            }
        }
    }

    /// One paragraph or heading: its lines, the drop cap beside the
    /// first of them, and where a page may end between them.
    fn paragraph(&mut self, id: NodeId, inlines: &[Inline], x: f32, measure: f32) {
        let computed = self.styles().style(id).clone();
        let start = self.open(&computed, inlines, x, measure);
        let (x, measure) = computed.content_box(x, measure);

        let style = computed.paragraph();
        let hyphenate = computed.hyphens == Hyphens::Auto;
        let options = LineBreakOptions {
            hyphenate,
            patterns: if hyphenate {
                self.paginator.patterns()
            } else {
                Patterns::NONE
            },
            justify: computed.text_align == TextAlign::Justify,
            inter_character: computed.text_justify == TextJustify::InterCharacter,
            hanging: computed.hanging_punctuation,
        };
        let cap = self.paginator.drop_cap(id, &computed, inlines);
        let full = Span::band(0.0, measure);
        let spec = match &cap {
            Some((cap, _)) => Measure::new(
                vec![Span::ending(measure, measure - cap.reserved); cap.lines],
                full,
            ),
            // An indent is a shorter first line. A cap outranks it:
            // the first line is already displaced, and a book does
            // not indent the paragraph a chapter opens with.
            None if computed.text_indent != 0.0 => Measure::new(
                vec![Span::ending(measure, measure - computed.text_indent)],
                full,
            ),
            None => Measure::uniform(measure),
        };
        // The letter the cap holds is passed over below, so a drop
        // cap and a first line over the same paragraph divide it:
        // `::first-letter` has the cap, `::first-line` the rest of
        // the line beside it.
        let opening = Opening {
            first_line: self.styles().opening_line(id),
            taken: cap.as_ref().map_or(0, |(_, taken)| *taken),
        };
        let lines = self.paginator.lines.layout_styled(
            inlines,
            style,
            self.styles(),
            &spec,
            options,
            opening,
        );

        let count = lines.len();
        let sunk = cap
            .as_ref()
            .map(|(cap, _)| cap.lines.min(count))
            .unwrap_or(0);
        // The cap's baseline is the last sunk line's, which is only
        // known once the lines are broken.
        let drop = if sunk > 0 {
            lines[1..sunk]
                .iter()
                .map(|line| line.box_.height)
                .sum::<f32>()
                + lines[sunk - 1].box_.baseline
                - lines[0].box_.baseline
        } else {
            0.0
        };
        let mut cap = cap.map(|(cap, _)| DropCap {
            line: cap.line,
            x,
            drop,
        });

        let (orphans, widows) = (computed.orphans as usize, computed.widows as usize);
        let mut first = true;
        let mut slot = 0;
        for (index, mut line) in lines.into_iter().enumerate() {
            let last = line.spans.len() - 1;
            // A band's slack is at the end of its last span, so that
            // is where alignment moves the text.
            let offset = align_offset(
                computed.text_align,
                self.paginator.span_width(&line, last),
                spec.at(slot + last).width,
            );
            line.spans[last].offset += offset;
            let origin = spec.at(slot).origin;
            slot += line.spans.len();
            let inner = if index < orphans || count - index < widows || index < sunk {
                BreakPoint::Forbidden
            } else {
                BreakPoint::Allowed
            };
            let height = line.box_.height;
            let protrusion = line.protrusion;
            let piece = Piece::Line {
                line,
                cap: (index == 0).then(|| cap.take()).flatten(),
            };
            self.emit(&mut first, inner, x + origin - protrusion, height, piece);
        }
        self.close(&computed, start);
    }

    /// A thematic break: the ornament the cascade named, or the space
    /// it leaves when it names none.
    fn ornament(&mut self, style: &ComputedStyle, x: f32, measure: f32) {
        let (x, measure) = style.content_box(x, measure);
        let paragraph = style.paragraph();
        let ornament = match &style.content {
            Content::Text(text) if !text.is_empty() => {
                self.paginator.line_of(text, paragraph).map(|line| {
                    let offset =
                        align_offset(style.text_align, self.paginator.line_width(&line), measure);
                    (offset, line.box_.height, Piece::Line { line, cap: None })
                })
            }
            _ => None,
        };
        let (offset, height, piece) = ornament.unwrap_or_else(|| {
            (
                0.0,
                self.paginator.lines.strut(paragraph).height(),
                Piece::Blank,
            )
        });
        self.emit_one(x + offset, height, piece);
    }

    /// A block image, sized as CSS 2.1 §10.4 sizes a replaced element
    /// with no width or height of its own: its intrinsic size, scaled
    /// down when that does not fit the page.
    fn image(&mut self, style: &ComputedStyle, url: &str, origin: String, x: f32, measure: f32) {
        let Some((asset, intrinsic)) = self.paginator.assets.lookup(url) else {
            // A url the table probed and refused was complained about
            // there; one it was never offered is a host that supplied
            // no image for it at all.
            if !self.paginator.assets.probed(url) {
                self.paginator.warn(
                    format!("image {url}: no image was supplied for it; it is skipped"),
                    (!origin.is_empty()).then_some(origin),
                );
            }
            return;
        };
        let (x, measure) = style.content_box(x, measure);
        let available = self
            .paginator
            .styles
            .default_page()
            .geometry
            .content_size()
            .1;
        let (mut width, mut height) = intrinsic.size();
        let scale = |value: f32, from: f32, to: f32| {
            if from > 0.0 { value * to / from } else { value }
        };
        if width > measure {
            height = scale(height, width, measure);
            width = measure;
        }
        if height > available {
            self.paginator.warn(
                format!("image {url} is taller than the content box; scaled to fit"),
                (!origin.is_empty()).then_some(origin),
            );
            width = scale(width, height, available);
            height = available;
        }
        let offset = align_offset(style.text_align, width, measure);
        self.emit_one(
            x + offset,
            height,
            Piece::Image {
                width,
                height,
                asset,
            },
        );
    }
}

impl Paginator<'_> {
    /// The initial letter one paragraph opens with, and the inlines
    /// left after it is taken out.
    ///
    /// The sink is what `initial-letter` asked for: the cap's own cap
    /// height spans that many lines, so its baseline lands on the
    /// last of them and its top on the first line's cap height.
    fn drop_cap(
        &self,
        id: NodeId,
        computed: &ComputedStyle,
        inlines: &[Inline],
    ) -> Option<(Cap, usize)> {
        let cap_style = self.styles.first_letter(id)?;
        let sink = cap_style.initial_letter as usize;
        if sink < 2 {
            return None;
        }
        let initial = take_initial(inlines)?;
        let body = computed.paragraph();
        let cap_metrics = self.registry.metrics(cap_style.font_id)?;
        let cap_units = cap_height(cap_metrics);
        if cap_units <= 0.0 {
            return None;
        }
        let body_metrics = self.registry.metrics(body.font_id)?;
        let body_cap = cap_height(body_metrics) / body_metrics.units_per_em as f32 * body.size;
        let sunk = (sink - 1) as f32 * self.lines.strut(body).height() + body_cap;
        let style = ParagraphStyle {
            size: sunk * cap_metrics.units_per_em as f32 / cap_units,
            ..cap_style.paragraph()
        };
        let mut line = self.line_of(&initial.letter.to_string(), style)?;
        // The cap stands for the letter it was taken from, so a
        // cursor on the manuscript's first word lands on it.
        if let Some(run) = line.runs.first_mut() {
            run.origin = Some(initial.origin);
        }
        // A word space of the body text separates the cap from the
        // lines it is sunk into.
        let gutter = self
            .registry
            .char_glyph(body.font_id, ' ')
            .and_then(|glyph| self.registry.advance_width(body.font_id, glyph))
            .unwrap_or(0) as f32
            / body_metrics.units_per_em as f32
            * body.size;
        Some((
            Cap {
                reserved: self.line_width(&line) + gutter,
                line,
                lines: sink,
            },
            initial.taken,
        ))
    }
}

/// A face's cap height, falling back to its ascender when the file
/// declares none.
fn cap_height(metrics: crate::fonts::FontMetricsTable) -> f32 {
    if metrics.cap_height > 0 {
        metrics.cap_height as f32
    } else {
        metrics.ascender as f32
    }
}

/// Where a line of `width` starts inside a measure of `available`.
fn align_offset(align: TextAlign, width: f32, available: f32) -> f32 {
    match align {
        TextAlign::Left | TextAlign::Justify => 0.0,
        TextAlign::Right => (available - width).max(0.0),
        TextAlign::Center => ((available - width) / 2.0).max(0.0),
    }
}

/// The letter a drop cap is set from, where it was written, and how
/// far into the paragraph the rest of the prose starts.
struct Initial {
    letter: char,
    /// The node the letter came out of, and the bytes of it the cap
    /// holds: the letter and whatever space stood before it.
    origin: SourceRange,
    /// Bytes of the paragraph's text the cap holds.
    taken: usize,
}

/// The first character of a run of inlines, wherever the markup has
/// put it: a paragraph opening in italic still opens with a letter.
fn take_initial(inlines: &[Inline]) -> Option<Initial> {
    fn walk(inlines: &[Inline], before: &mut usize) -> Option<Initial> {
        for inline in inlines {
            match inline {
                Inline::Text { id, value, .. } | Inline::Code { id, value, .. } => {
                    let space = value.len() - value.trim_start().len();
                    if let Some(letter) = value[space..].chars().next() {
                        let held = (space + letter.len_utf8()) as u32;
                        return Some(Initial {
                            letter,
                            origin: SourceRange {
                                node: *id,
                                range: 0..held,
                            },
                            taken: *before + held as usize,
                        });
                    }
                    *before += value.len();
                }
                Inline::Emphasis { children, .. }
                | Inline::Strong { children, .. }
                | Inline::Link { children, .. } => {
                    if let Some(initial) = walk(children, before) {
                        return Some(initial);
                    }
                }
            }
        }
        None
    }
    walk(inlines, &mut 0)
}

/// One fragment placed on the page being built.
struct Placed {
    /// The section its content came out of. The fragment records it, so
    /// a fragment moved onto the next page counts toward the page it
    /// ends on rather than the one it was measured for.
    section: NodeId,
    /// The column of the page it landed in.
    column: u32,
    /// Top of its box, from its column's top.
    top: f32,
    /// Its own height.
    height: f32,
    /// Whether a page may end above it.
    break_before: BreakPoint,
    /// What it paints, already positioned on this page.
    items: Vec<DrawItem>,
    /// What it sets for the furniture of whichever page it ends on.
    marks: Option<Box<Marks>>,
    /// The decorated blocks it opens and closes.
    decorations: Option<Box<Decorations>>,
}

/// One decorated block resolved against one page: the border box it
/// takes there, and which of its edges the page boundary cut.
struct Painted {
    decoration: Decoration,
    /// Top of the border box, from the page content box's top.
    top: f32,
    /// Its bottom, the same way.
    bottom: f32,
    /// Whether the block began on an earlier page.
    cut_above: bool,
    /// Whether it goes on to the next one.
    cut_below: bool,
}

impl Painted {
    /// The rects this box paints, background first: `origin` is the
    /// page's content box.
    fn items(&self, origin: (f32, f32)) -> Vec<DrawItem> {
        let (x, y) = (origin.0 + self.decoration.x, origin.1 + self.top);
        let (w, h) = (self.decoration.width, self.bottom - self.top);
        if w <= 0.0 || h <= 0.0 {
            return Vec::new();
        }
        let mut items = Vec::new();
        if let Some(color) = self.decoration.background {
            items.push(DrawItem::Rect { x, y, w, h, color });
        }
        // `slice` leaves the two edges the break made open; `clone`
        // closes them.
        let closed = self.decoration.cloned;
        let border = self.decoration.border;
        let top = if self.cut_above && !closed {
            0.0
        } else {
            border.top
        };
        let bottom = if self.cut_below && !closed {
            0.0
        } else {
            border.bottom
        };
        // The corners fall to the horizontal edges: a filled rect is
        // all the display structure has, and a mitre is a path.
        let colors = self.decoration.colors;
        let mut rect = |x: f32, y: f32, w: f32, h: f32, color| {
            if w > 0.0 && h > 0.0 {
                items.push(DrawItem::Rect { x, y, w, h, color });
            }
        };
        rect(x, y, w, top, colors.top);
        rect(x, y + h - bottom, w, bottom, colors.bottom);
        let side = h - top - bottom;
        rect(x, y + top, border.left, side, colors.left);
        rect(
            x + w - border.right,
            y + top,
            border.right,
            side,
            colors.right,
        );
        items
    }
}

/// What is recorded about one finished page: the master to ask for,
/// the running strings as they stood when it opened, and the folio it
/// restarts at.
pub(crate) struct PageInfo {
    slot: PageSlot,
    strings: Strings,
    reset: Option<u32>,
    /// Items the flow itself painted, before any furniture.
    content_items: usize,
}

/// Pages as fragmentation settled them, and what each one needs to
/// paint its furniture. The two travel together, because a folio is a
/// fact about where a page landed rather than about what is on it.
pub(crate) struct Paged {
    pub(crate) pages: Vec<Page>,
    pub(crate) infos: Vec<PageInfo>,
}

/// The flow: fragments in, pages out.
///
/// The page being built is a list of placed fragments rather than a
/// finished structure, because a fragment that does not fit can
/// push the ones above it onto the next page — moving what is already
/// painted, never measuring it again.
struct Flow<'a, 'p> {
    paginator: &'p Paginator<'a>,
    pages: Vec<Page>,
    infos: Vec<PageInfo>,
    placed: Vec<Placed>,
    slot: PageSlot,
    /// The running strings as the page being built opened. The flow
    /// only advances them when a page closes, so this is what
    /// `string()` reads.
    strings: Strings,
    /// The slot the next page opens with: a section waiting for a
    /// page of its own.
    pending_slot: Option<PageSlot>,
    /// The section whose fragments are being placed.
    section: NodeId,
    /// Bottom of what is placed, from the column's top.
    cursor: f32,
    /// Height of the column being filled, which is the content box's.
    height: f32,
    /// The column being filled, counting from the leading edge.
    column: u32,
    /// How many the page divides into.
    columns: u32,
    /// Where in `placed` the column being filled began. A break backs
    /// up to a fragment of this column, never past its head.
    column_start: usize,
    /// The decorated blocks the page being built opened with,
    /// outermost first: a block the page before it did not finish.
    carried: Vec<Decoration>,
}

impl<'a, 'p> Flow<'a, 'p> {
    fn new(paginator: &'p Paginator<'a>) -> Flow<'a, 'p> {
        let slot = PageSlot {
            name: None,
            first: true,
            blank: false,
        };
        let geometry = paginator.master(0, &slot).geometry;
        Flow {
            paginator,
            pages: Vec::new(),
            infos: Vec::new(),
            placed: Vec::new(),
            slot,
            strings: Strings::new(),
            pending_slot: None,
            section: NodeId::UNASSIGNED,
            cursor: 0.0,
            height: geometry.content_size().1,
            column: 0,
            columns: geometry.column_count(),
            column_start: 0,
            carried: Vec::new(),
        }
    }

    /// Flows one section. Its page name and `@page :first` master are
    /// claimed by the page it opens — when it opens one at all: a
    /// section that breaks `auto` continues where the last left off.
    fn section(&mut self, section: &Section, fragments: &[Fragment]) {
        let style = self.paginator.styles.style(section.id);
        self.section = section.id;
        self.pending_slot = Some(PageSlot {
            name: style.page.clone(),
            first: true,
            blank: false,
        });
        for fragment in fragments {
            self.place(fragment);
            self.pending_slot = None;
        }
    }

    /// Places one fragment, ending columns and pages as its break
    /// point demands.
    fn place(&mut self, fragment: &Fragment) {
        if let BreakPoint::Forced(wanted) = fragment.break_before {
            match wanted {
                Break::Column => self.break_column(),
                _ => {
                    self.close();
                    if let Break::Side(side) = wanted {
                        self.square_to(side);
                    }
                }
            }
        }
        // Nothing laid on the page yet means the page the section is
        // waiting for is this one.
        if self.placed.is_empty()
            && let Some(slot) = self.pending_slot.take()
        {
            self.slot = slot;
            self.remaster();
        }
        // A fragment that does not fit ends the column. Where it ends
        // is the last point a break was allowed — which may be
        // several fragments back, and may be nowhere, in which case
        // the break falls here whatever the cascade wanted.
        let mut forced = false;
        loop {
            let opening = self.column_empty();
            let lead = if opening { 0.0 } else { fragment.lead };
            if opening || self.cursor + lead + fragment.fixed + fragment.height <= self.height {
                self.emit(fragment, lead);
                return;
            }
            let cut = if forced || fragment.break_before != BreakPoint::Forbidden {
                self.placed.len()
            } else {
                self.back_up().unwrap_or(self.placed.len())
            };
            self.carry(cut);
            forced = true;
        }
    }

    /// Whether nothing stands in the column being filled.
    fn column_empty(&self) -> bool {
        self.placed.len() == self.column_start
    }

    /// The last place above the foot of the column where a break was
    /// allowed. Never its head: a column that carries everything on
    /// it into the next one makes no progress.
    fn back_up(&self) -> Option<usize> {
        (self.column_start + 1..self.placed.len())
            .rev()
            .find(|index| self.placed[*index].break_before == BreakPoint::Allowed)
    }

    /// Ends the column at `cut`, carrying what was below into the
    /// next one, which is the next page's first where the column that
    /// ended was the page's last. Carried fragments move; they are
    /// never measured again.
    fn carry(&mut self, cut: usize) {
        let mut carried = self.placed.split_off(cut);
        let (from_x, from_y) = self.origin();
        self.advance();
        let (to_x, to_y) = self.origin();
        let Some(head) = carried.first().map(|placed| placed.top) else {
            return;
        };
        // The carried group starts at the head of the fresh column,
        // and the space that was above it there is dropped.
        let (dx, dy) = (to_x - from_x, to_y - from_y - head);
        let column = self.column;
        for placed in &mut carried {
            placed.top -= head;
            placed.column = column;
            shift(&mut placed.items, dx, dy);
        }
        self.cursor = carried
            .last()
            .map(|placed| placed.top + placed.height)
            .unwrap_or(0.0);
        self.placed.append(&mut carried);
    }

    /// Moves to the next column, or ends the page when the column
    /// that filled was its last.
    fn advance(&mut self) {
        if self.column + 1 < self.columns {
            self.column += 1;
            self.cursor = 0.0;
            self.column_start = self.placed.len();
        } else {
            self.close();
        }
    }

    /// What `break-before: column` asks for: the next column, unless
    /// this one is still empty, which is already the column it asks
    /// for.
    fn break_column(&mut self) {
        if !self.column_empty() {
            self.advance();
        }
    }

    /// Paints one fragment onto the page being built.
    fn emit(&mut self, fragment: &Fragment, lead: f32) {
        let (x, y) = self.origin();
        let top = self.cursor + lead + fragment.fixed;
        let items = match &fragment.piece {
            Piece::Line { line, cap } => {
                let baseline = y + top + line.box_.baseline;
                let mut items = self.paginator.text_items(line, x + fragment.x, baseline);
                if let Some(cap) = cap {
                    items.append(&mut self.paginator.text_items(
                        &cap.line,
                        x + cap.x,
                        baseline + cap.drop,
                    ));
                }
                items
            }
            Piece::Image {
                width,
                height,
                asset,
            } => vec![DrawItem::Image {
                x: x + fragment.x,
                y: y + top,
                w: *width,
                h: *height,
                asset: *asset,
            }],
            Piece::Blank => Vec::new(),
        };
        self.cursor = top + fragment.height;
        self.placed.push(Placed {
            section: self.section,
            column: self.column,
            top,
            height: fragment.height,
            break_before: fragment.break_before,
            items,
            marks: fragment.marks.clone(),
            decorations: fragment.decorations.clone(),
        });
    }

    /// Resolves the decorations over the page being closed into the
    /// rects they paint there, column by column.
    ///
    /// A column boundary cuts a block the way a page boundary does,
    /// so each column resolves on its own and what is still open at
    /// the foot of one carries into the next.
    fn decorate(&mut self, placed: &[Placed]) -> Vec<DrawItem> {
        let mut items = Vec::new();
        let mut start = 0;
        for index in 1..=placed.len() {
            if index < placed.len() && placed[index].column == placed[start].column {
                continue;
            }
            let column = placed[start].column;
            items.extend(self.decorate_column(&placed[start..index], column));
            start = index;
        }
        items
    }

    /// The same over one column.
    ///
    /// A block whose first fragment landed in this column has its top
    /// edge here, and one whose last fragment did has its bottom; the
    /// ranges are contiguous, so a block with neither covers every
    /// fragment the column holds. What is still open when the column
    /// closes carries into the next.
    fn decorate_column(&mut self, placed: &[Placed], column: u32) -> Vec<DrawItem> {
        let mut boxes: Vec<Painted> = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        for decoration in self.carried.drain(..) {
            open.push(boxes.len());
            boxes.push(Painted {
                decoration,
                top: 0.0,
                bottom: 0.0,
                cut_above: true,
                cut_below: true,
            });
        }
        for entry in placed {
            let Some(decorations) = &entry.decorations else {
                continue;
            };
            for decoration in &decorations.opens {
                open.push(boxes.len());
                boxes.push(Painted {
                    top: entry.top - decoration.above,
                    decoration: decoration.clone(),
                    bottom: 0.0,
                    cut_above: false,
                    cut_below: true,
                });
            }
            for _ in 0..decorations.closes {
                let Some(index) = open.pop() else { continue };
                boxes[index].bottom = entry.top + entry.height + boxes[index].decoration.below;
                boxes[index].cut_below = false;
            }
        }
        let last = placed
            .last()
            .map(|entry| entry.top + entry.height)
            .unwrap_or(0.0);
        for index in open {
            boxes[index].bottom = last;
            self.carried.push(boxes[index].decoration.clone());
        }
        let origin = self.column_origin(column);
        boxes.iter().flat_map(|box_| box_.items(origin)).collect()
    }

    /// The rules down the gutters of the page being closed: one down
    /// each gutter the flow filled past, over the height of the
    /// taller of the two columns it divides.
    ///
    /// A page the flow left in one column paints no rule.
    fn rules(&self, placed: &[Placed]) -> Vec<DrawItem> {
        let geometry = self.paginator.master(self.pages.len(), &self.slot).geometry;
        let width = geometry.columns.rule.used();
        if width <= 0.0 || self.columns < 2 {
            return Vec::new();
        }
        let mut feet = vec![0.0f32; self.columns as usize];
        let mut filled = vec![false; self.columns as usize];
        for entry in placed {
            let column = entry.column as usize;
            feet[column] = feet[column].max(entry.top + entry.height);
            filled[column] = true;
        }
        let (_, top) = geometry.content_origin();
        let color = self.paginator.styles.root().color;
        (1..self.columns as usize)
            .filter(|column| filled[*column])
            .map(|column| {
                let gutter = geometry.column_origin(column as u32).0 - geometry.columns.gap;
                DrawItem::Rect {
                    x: gutter + (geometry.columns.gap - width) / 2.0,
                    y: top,
                    w: width,
                    h: feet[column - 1].max(feet[column]),
                    color,
                }
            })
            .collect()
    }

    /// Ends the page being built, if anything is on it.
    ///
    /// What the page's fragments set takes effect here, not where
    /// they were placed: a fragment moved onto the next page sets
    /// its strings there instead, so a page's furniture only ever
    /// reads what stood on it.
    fn close(&mut self) {
        if self.placed.is_empty() {
            return;
        }
        let opened = self.strings.clone();
        let mut reset = None;
        let placed = std::mem::take(&mut self.placed);
        // Backgrounds, borders and column rules go in front of the
        // page's text: `DrawItem` order is paint order, and the
        // display structure has no layers.
        let mut items = self.decorate(&placed);
        items.append(&mut self.rules(&placed));
        let mut sections: Vec<NodeId> = Vec::new();
        for placed in placed {
            if sections.last() != Some(&placed.section) {
                sections.push(placed.section);
            }
            if let Some(marks) = placed.marks {
                for (name, value) in marks.strings {
                    self.strings.insert(name, value);
                }
                reset = reset.or(marks.page_number);
            }
            items.extend(placed.items);
        }
        let mut page = self.paginator.blank_page(&self.slot);
        page.side = Side::of_number(self.pages.len() as u32 + 1);
        page.sections = sections;
        page.items = items;
        let content_items = page.items.len();
        self.pages.push(page);
        self.infos.push(PageInfo {
            slot: self.slot.clone(),
            strings: opened,
            reset,
            content_items,
        });
        self.slot = self.pending_slot.take().unwrap_or(PageSlot {
            first: false,
            ..self.slot.clone()
        });
        self.cursor = 0.0;
        self.column = 0;
        self.column_start = 0;
        self.remaster();
    }

    /// Ships blank leaves until the next page falls on `side`.
    fn square_to(&mut self, side: Side) {
        while Side::of_number(self.pages.len() as u32 + 1) != side {
            let blank = PageSlot {
                name: None,
                first: false,
                blank: true,
            };
            self.pages.push(self.paginator.blank_page(&blank));
            self.infos.push(PageInfo {
                slot: blank,
                strings: self.strings.clone(),
                reset: None,
                content_items: 0,
            });
        }
        self.remaster();
    }

    /// The column being filled, in page coordinates.
    fn origin(&self) -> (f32, f32) {
        self.column_origin(self.column)
    }

    /// One column of the page being built, in page coordinates.
    fn column_origin(&self, column: u32) -> (f32, f32) {
        self.paginator
            .master(self.pages.len(), &self.slot)
            .geometry
            .column_origin(column)
    }

    fn remaster(&mut self) {
        let geometry = self.paginator.master(self.pages.len(), &self.slot).geometry;
        self.height = geometry.content_size().1;
        self.columns = geometry.column_count();
    }

    fn finish(mut self) -> Paged {
        self.close();
        Paged {
            pages: self.pages,
            infos: self.infos,
        }
    }
}

/// Moves already-painted items: what moving a fragment to the next
/// page comes to.
fn shift(items: &mut [DrawItem], dx: f32, dy: f32) {
    for item in items {
        match item {
            DrawItem::Text { x, y, glyphs, .. } => {
                *x += dx;
                *y += dy;
                for glyph in glyphs {
                    glyph.x += dx;
                }
            }
            DrawItem::Rect { x, y, .. } | DrawItem::Image { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
        }
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
    use crate::content::{Attributes, HeadingLevel, Inline, NodeId, SourcePos};
    use crate::style::StyleTree;

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
    }

    fn text(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    fn heading(value: &str) -> Block {
        Block::Heading {
            id: NodeId::UNASSIGNED,
            level: HeadingLevel::H1,
            inlines: vec![text(value)],
            attributes: Attributes::default(),
            position: Some(SourcePos { line: 1, column: 1 }),
            span: None,
        }
    }

    fn paragraph(value: &str) -> Block {
        Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![text(value)],
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    fn section(blocks: Vec<Block>) -> Section {
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
            .expect("the default page has a folio")
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

    /// The same, with author CSS cascading over the built-in sheet.
    fn paginate_styled(css: &str, sections: Vec<Section>) -> Vec<Page> {
        let book = book_of(sections);
        let styles = styled(css, &book);
        Paginator::new(registry(), &styles).paginate(&book)
    }

    /// One book's styling with author CSS over the built-in sheet.
    fn styled(css: &str, book: &Book) -> StyleTree {
        crate::style::Stylesheets::parse(&[crate::style::Source::author("test.css", css)])
            .compile(book, registry())
    }

    /// The page box one sheet computes for a chapter page, which is
    /// the master the fixture sections resolve to. The situation is
    /// the page's own: mirrored margins put the columns of a verso
    /// page at different offsets from a recto's.
    fn styled_geometry(css: &str, situation: Situation) -> crate::style::PageGeometry {
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
    fn page_geometry(css: &str, page: &Page) -> crate::style::PageGeometry {
        styled_geometry(css, Situation::Body(page.side))
    }

    /// Prose enough to break over several lines.
    fn prose() -> Block {
        paragraph(&"a quiet sentence of prose ".repeat(6))
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

    /// A page divided in two, with a gutter wide enough to tell the
    /// columns apart by where a line starts.
    const TWO_COLUMNS: &str = "@page { column-count: 2; column-gap: 18pt }";

    /// A two-column page reads in column order: every line of the
    /// first column comes before every line of the second, and the
    /// second starts back at the top of the page.
    #[test]
    fn a_two_column_page_fills_one_column_before_the_next() {
        let pages = paginate_styled(
            TWO_COLUMNS,
            vec![section((0..12).map(|_| prose()).collect())],
        );
        let geometry = styled_geometry(TWO_COLUMNS, Situation::First(Side::Recto));
        let measure = geometry.measure();
        let (left, top) = geometry.content_origin();
        let second = geometry.column_origin(1).0;
        assert_eq!(measure, (geometry.content_size().0 - 18.0) / 2.0);
        let lines = content_lines(&pages[0]);
        let column_of = |runs: &Vec<Run<'_>>| {
            if runs.iter().all(|(x, _, _)| *x < second) {
                0
            } else {
                1
            }
        };
        let columns: Vec<usize> = lines.iter().map(|(_, runs)| column_of(runs)).collect();
        let turn = columns
            .iter()
            .position(|column| *column == 1)
            .expect("the prose reaches the second column");
        assert!(columns[..turn].iter().all(|column| *column == 0));
        assert!(columns[turn..].iter().all(|column| *column == 1));
        // The second column opens at the top of the page, below the
        // first column's last line.
        assert!(lines[turn].0 < lines[turn - 1].0);
        assert!(lines[turn].0 > top);
        for (index, (_, runs)) in lines.iter().enumerate() {
            let origin = if columns[index] == 0 { left } else { second };
            for (x, _, _) in runs {
                assert!(
                    *x >= origin - 1e-3,
                    "line {index} starts left of its column"
                );
            }
        }
    }

    /// The lines of one page grouped by the column they were set
    /// in, first column first: the tagged first word of each line,
    /// the way `tagged_lines` reads a page.
    fn tagged_columns(page: &Page, geometry: crate::style::PageGeometry) -> Vec<Vec<String>> {
        let mut columns = vec![Vec::new(); geometry.column_count() as usize];
        for (_, runs) in content_lines(page) {
            let x = runs[0].0;
            let column = (0..geometry.column_count())
                .rev()
                .find(|column| x >= geometry.column_origin(*column).0 - 1e-3)
                .unwrap_or(0);
            columns[column as usize].push(
                runs[0]
                    .2
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        columns
    }

    /// The page ends after its last column: a book that spills onto a
    /// second page filled both columns of the first, each of them to
    /// within a line of its foot.
    #[test]
    fn a_page_ends_when_its_last_column_does() {
        let pages = paginate_styled(
            TWO_COLUMNS,
            vec![section((0..40).map(|_| prose()).collect())],
        );
        assert!(pages.len() > 1, "the book spills onto a second page");
        let (_, top) = styled_geometry(TWO_COLUMNS, Situation::First(Side::Recto)).content_origin();
        let foot = top
            + styled_geometry(TWO_COLUMNS, Situation::First(Side::Recto))
                .content_size()
                .1;
        let line = body_size() * ua().root().line_height;
        for (index, page) in pages[..pages.len() - 1].iter().enumerate() {
            let geometry = page_geometry(TWO_COLUMNS, page);
            for (column, lines) in tagged_columns(page, geometry).iter().enumerate() {
                assert!(
                    !lines.is_empty(),
                    "page {}: column {column} is empty",
                    index + 1
                );
            }
            let last = content_lines(page)
                .last()
                .expect("a filled page has lines")
                .0;
            assert!(
                last > foot - 2.0 * line,
                "page {}: the last column stopped {} short of the foot",
                index + 1,
                foot - last
            );
        }
    }

    /// `break-before: column` on a heading opens the next column. The
    /// page it opens on is the same one, and the prose before it is
    /// still in the column it was set in.
    #[test]
    fn break_before_column_opens_a_column_rather_than_a_page() {
        let sections = || vec![section(vec![prose(), heading("Second"), prose(), prose()])];
        let pages = paginate_styled(
            &format!("{TWO_COLUMNS} h1 {{ break-before: column }}"),
            sections(),
        );
        assert_eq!(pages.len(), 1, "a column break does not turn the page");
        let geometry = page_geometry(TWO_COLUMNS, &pages[0]);
        let columns = tagged_columns(&pages[0], geometry);
        assert_eq!(columns[1].first().map(String::as_str), Some("Second"));
        assert!(!columns[0].iter().any(|line| line == "Second"));
    }

    /// The same heading with `break-before: page` still turns the
    /// page, so the column break is the weaker of the two rather than
    /// the only one left.
    #[test]
    fn break_before_page_still_turns_the_page_on_a_divided_box() {
        let pages = paginate_styled(
            &format!("{TWO_COLUMNS} h1 {{ break-before: page }}"),
            vec![section(vec![prose(), heading("Second"), prose()])],
        );
        assert_eq!(pages.len(), 2);
        let geometry = page_geometry(TWO_COLUMNS, &pages[1]);
        let columns = tagged_columns(&pages[1], geometry);
        assert_eq!(columns[0].first().map(String::as_str), Some("Second"));
    }

    /// Acceptance: orphans and widows hold at a column boundary the
    /// way they hold at a page boundary. A line that would stand
    /// alone at the head of a column takes its paragraph with it.
    #[test]
    fn orphans_and_widows_hold_at_every_column_boundary() {
        let pages = paginate_styled(TWO_COLUMNS, vec![section(tagged_prose(60))]);
        let columns: Vec<Vec<String>> = pages
            .iter()
            .flat_map(|page| tagged_columns(page, page_geometry(TWO_COLUMNS, page)))
            .collect();
        assert_orphans_and_widows_over(&columns, "column", 2, 2);
    }

    /// `break-inside: avoid` holds inside a column: a quotation the
    /// rest of a column cannot take moves whole into the next one
    /// rather than splitting across the gutter.
    #[test]
    fn break_inside_avoid_keeps_a_block_in_one_column() {
        let blocks: Vec<Block> = (0..12)
            .flat_map(|index| {
                let quoted = format!("q{index:02}");
                [
                    paragraph(&vec![format!("p{index:02}"); (5 + index % 7) * 18].join(" ")),
                    quote(vec![paragraph(&vec![quoted; 30].join(" "))]),
                ]
            })
            .collect();
        let pages = paginate_styled(
            &format!("{TWO_COLUMNS} blockquote {{ break-inside: avoid }}"),
            vec![section(blocks)],
        );
        let mut seen: BTreeMap<String, Vec<(u32, usize)>> = BTreeMap::new();
        for page in &pages {
            let geometry = page_geometry(TWO_COLUMNS, page);
            for (column, lines) in tagged_columns(page, geometry).iter().enumerate() {
                for token in lines.iter().filter(|token| token.starts_with('q')) {
                    let at = (page.number, column);
                    let places = seen.entry(token.clone()).or_default();
                    if places.last() != Some(&at) {
                        places.push(at);
                    }
                }
            }
        }
        assert!(seen.len() >= 8, "only {} quotations to check", seen.len());
        for (token, places) in &seen {
            assert_eq!(
                places.len(),
                1,
                "{token} is set over {places:?} rather than in one column",
            );
        }
    }

    /// `break-after: column` closes the column under the block that
    /// asks for it, and what follows opens the next one.
    #[test]
    fn break_after_column_closes_the_column_under_it() {
        let pages = paginate_styled(
            &format!("{TWO_COLUMNS} h1 {{ break-after: column }}"),
            vec![section(vec![heading("Opening"), prose(), prose()])],
        );
        assert_eq!(pages.len(), 1);
        let columns = tagged_columns(&pages[0], page_geometry(TWO_COLUMNS, &pages[0]));
        assert_eq!(columns[0], vec!["Opening".to_string()]);
        assert!(!columns[1].is_empty(), "the prose opens the second column");
    }

    /// A rule paints down the gutter, centred in it, from the top of
    /// the content box to the foot of the columns it divides.
    #[test]
    fn a_column_rule_paints_centred_in_the_gutter() {
        let css =
            format!("{TWO_COLUMNS} @page {{ column-rule-style: solid; column-rule-width: 1pt }}");
        let pages = paginate_styled(&css, vec![section((0..12).map(|_| prose()).collect())]);
        let geometry = page_geometry(&css, &pages[0]);
        let rects = rects(&pages[0]);
        assert_eq!(rects.len(), 1, "two columns, one gutter, one rule");
        let (x, y, w, h, color) = rects[0];
        assert_eq!(w, 1.0);
        assert_eq!(color, ua().root().color);
        let gutter = geometry.column_origin(1).0 - geometry.columns.gap;
        assert_eq!(x + w / 2.0, gutter + geometry.columns.gap / 2.0);
        let (_, top) = geometry.content_origin();
        assert_eq!(y, top);
        let last = content_lines(&pages[0])
            .last()
            .expect("the page has lines")
            .0;
        assert!(y + h >= last - 1e-3, "the rule reaches the last line");
        assert!(y + h <= top + geometry.content_size().1 + 1e-3);
    }

    /// A page the flow left in one column divides nothing, and paints
    /// no rule.
    #[test]
    fn a_one_column_page_paints_no_rule() {
        let css = format!("{TWO_COLUMNS} @page {{ column-rule-style: solid }}");
        let short = paginate_styled(&css, vec![section(vec![paragraph("One short line.")])]);
        assert!(rects(&short[0]).is_empty());
        let undivided = paginate_styled(
            "@page { column-rule-style: solid; column-rule-width: 1pt }",
            vec![section((0..12).map(|_| prose()).collect())],
        );
        assert!(rects(&undivided[0]).is_empty());
    }

    /// Furniture belongs to the page: the folio of a two-column page
    /// sits where the folio of the same page undivided does.
    #[test]
    fn columns_leave_the_furniture_where_it_was() {
        let sections = || vec![section((0..40).map(|_| prose()).collect())];
        let folio = |pages: &[Page], index: usize| {
            pages[index]
                .items
                .iter()
                .find_map(|item| match item {
                    DrawItem::Text { x, y, size, .. } if *size == folio_size() => Some((*x, *y)),
                    _ => None,
                })
                .expect("a body page carries a folio")
        };
        let divided = paginate_styled(TWO_COLUMNS, sections());
        let undivided = paginate(sections());
        assert!(divided.len() > 1 && undivided.len() > 1);
        assert_eq!(folio(&divided, 1), folio(&undivided, 1));
    }

    /// A quotation set with padding on all four edges and a rule down
    /// its leading edge: its lines start inside both, and the measure
    /// they break to gives up both on the left and the padding alone
    /// on the right.
    #[test]
    fn padding_and_a_border_move_the_leading_edge_and_narrow_the_measure() {
        let sections = vec![section(vec![quote(vec![prose()])])];
        let plain = paginate_styled("blockquote { margin: 0 }", sections.clone());
        let boxed = paginate_styled(
            "blockquote { margin: 0; padding: 12pt; border-left: 2pt solid }",
            sections,
        );
        let measure = master(Situation::First(Side::Recto)).geometry.measure();
        let left = |pages: &[Page]| content_items(&pages[0])[0].0;
        assert_eq!(left(&boxed) - left(&plain), 14.0);
        let widest = |pages: &[Page]| {
            content_lines(&pages[0])
                .iter()
                .map(|(_, runs)| runs.iter().map(|(x, _, _)| *x).fold(0.0f32, f32::max))
                .fold(0.0f32, f32::max)
        };
        assert!(widest(&boxed) <= left(&plain) + measure - 12.0);
    }

    /// The rule down a quotation reaches every line it sets: one rect,
    /// the height of the border box, over the whole quote.
    #[test]
    fn a_rule_reaches_the_full_height_of_the_box() {
        let pages = paginate_styled(
            "blockquote { margin: 0; padding: 12pt; border-left: 2pt solid }",
            vec![section(vec![quote(vec![prose()])])],
        );
        let rect = rects(&pages[0]);
        assert_eq!(rect.len(), 1, "one edge, one rect");
        let (x, y, w, h, color) = rect[0];
        assert_eq!(w, 2.0);
        assert_eq!(color, Color::BLACK);
        let lines = content_lines(&pages[0]);
        let (first, last) = (lines[0].0, lines[lines.len() - 1].0);
        assert!(y < first, "the rule starts above the first baseline");
        assert!(y + h > last, "and ends below the last");
        let origin = master(Situation::First(Side::Recto))
            .geometry
            .content_origin();
        assert_eq!(x, origin.0);
    }

    /// A rule under a heading sits below its last baseline, and the
    /// prose under it moves down by the rule and the padding
    /// together.
    #[test]
    fn a_rule_under_a_heading_moves_the_prose_below_it() {
        let sections = || vec![section(vec![heading("Chapter One"), prose()])];
        let plain = paginate_styled("h1 { margin: 0 }", sections());
        let ruled = paginate_styled(
            "h1 { margin: 0; padding-bottom: 6pt; border-bottom: 1pt solid }",
            sections(),
        );
        let opening = |pages: &[Page]| content_lines(&pages[0])[1].0;
        assert_eq!(opening(&ruled) - opening(&plain), 7.0);
        let rect = rects(&ruled[0]);
        assert_eq!(rect.len(), 1);
        let (_, y, _, h, _) = rect[0];
        assert_eq!(h, 1.0);
        let title = content_lines(&ruled[0])[0].0;
        assert!(y > title, "the rule sits below the heading's baseline");
        assert!(y + h < opening(&ruled), "and above the prose");
    }

    /// A top border stops a block's top margin collapsing with its
    /// first child's: both are set, rather than the larger of the two.
    #[test]
    fn a_top_border_stops_the_margins_collapsing() {
        let sections = || vec![section(vec![paragraph("Above."), quote(vec![prose()])])];
        let collapsed = paginate_styled(
            "blockquote { margin: 20pt 0 } blockquote p { margin-top: 10pt }",
            sections(),
        );
        let held = paginate_styled(
            "blockquote { margin: 20pt 0; border-top: 1pt solid } blockquote p { margin-top: 10pt }",
            sections(),
        );
        let gap = |pages: &[Page]| {
            let lines = content_lines(&pages[0]);
            lines[1].0 - lines[0].0
        };
        assert_eq!(gap(&held) - gap(&collapsed), 11.0);
    }

    /// A tinted quote broken over pages paints on every one of them,
    /// each over the fragments that page holds. `slice` leaves the
    /// two edges a break made open; `clone` closes them.
    #[test]
    fn a_box_split_across_pages_paints_on_every_one() {
        let tint = Color::rgb(0xee, 0xee, 0xee);
        let sheet = |extra: &str| {
            format!(
                "blockquote {{ margin: 0; background-color: #eeeeee; border: 1pt solid; {extra} }}"
            )
        };
        let quoted = || vec![section(vec![quote((0..40).map(|_| prose()).collect())])];
        /// The rules across a border box: its top and bottom edges,
        /// which are the ones a page break opens.
        fn rules(page: &Page) -> usize {
            rects(page)
                .iter()
                .filter(|(_, _, w, h, color)| w > h && *color == Color::BLACK)
                .count()
        }

        let sliced = paginate_styled(&sheet(""), quoted());
        assert!(sliced.len() >= 3, "the quote has to break twice");
        let last = sliced.len() - 1;
        assert!(
            sliced
                .iter()
                .all(|page| rects(page).iter().any(|rect| rect.4 == tint)),
            "every page the quote covers is tinted"
        );
        assert_eq!(rules(&sliced[0]), 1, "the top edge, where it began");
        assert_eq!(rules(&sliced[1]), 0, "both edges cut");
        assert_eq!(rules(&sliced[last]), 1, "the bottom edge, where it ended");

        let cloned = paginate_styled(&sheet("box-decoration-break: clone"), quoted());
        assert_eq!(cloned.len(), sliced.len());
        for page in &cloned {
            assert_eq!(rules(page), 2, "each piece closed on both edges");
        }
    }

    /// Nothing painted where nothing was asked for: a sheet that
    /// names no padding, border or background lays out the same
    /// display structure it did before there was a box model.
    #[test]
    fn a_book_that_asks_for_no_box_paints_no_rects() {
        let pages = paginate(vec![section(vec![
            heading("Chapter One"),
            prose(),
            quote(vec![prose()]),
            scene_break(),
            prose(),
        ])]);
        assert!(pages.iter().all(|page| rects(page).is_empty()));
    }

    fn quote(blocks: Vec<Block>) -> Block {
        Block::Blockquote {
            id: NodeId::UNASSIGNED,
            blocks,
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    fn scene_break() -> Block {
        Block::ThematicBreak {
            id: NodeId::UNASSIGNED,
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    /// One page's content paint ops, folio excluded: `(x, baseline,
    /// size, text)` in paint order.
    fn content_items(page: &Page) -> Vec<(f32, f32, f32, &str)> {
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
    type Run<'a> = (f32, f32, &'a str);

    /// One content line: its baseline, and the runs sharing it.
    type ContentLine<'a> = (f32, Vec<Run<'a>>);

    /// The content lines of one page: the paint ops grouped by the
    /// baseline they share, in order down the page.
    fn content_lines(page: &Page) -> Vec<ContentLine<'_>> {
        let mut lines: Vec<ContentLine<'_>> = Vec::new();
        for (x, y, size, text) in content_items(page) {
            match lines.last_mut() {
                Some((baseline, runs)) if (*baseline - y).abs() < 1e-3 => {
                    runs.push((x, size, text))
                }
                _ => lines.push((y, vec![(x, size, text)])),
            }
        }
        lines
    }

    /// Where the content on one baseline ends: the far edge of the
    /// last glyph on it, which is where a flush right edge falls.
    fn right_edge(page: &Page, baseline: f32) -> f32 {
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
    fn origin_of(page: &Page) -> (f32, f32) {
        master(Situation::Body(page.side)).geometry.content_origin()
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

    /// A run's `text` is what was drawn, and its `source` is what was
    /// written. The two painters read different fields for different
    /// reasons: one draws characters and hands `text` to a browser,
    /// the other maps glyphs back and reads `source`. A run with
    /// only the manuscript in it would have the preview set a
    /// chapter title in the case the export does not.
    #[test]
    fn a_run_says_both_what_was_drawn_and_what_was_written() {
        let pages = paginate_styled(
            "h1 { text-transform: uppercase }",
            vec![section(vec![heading("A Voyage to Lilliput")])],
        );
        let title = pages[0]
            .items
            .iter()
            .find_map(|item| match item {
                DrawItem::Text {
                    text,
                    source,
                    source_map,
                    glyphs,
                    ..
                } => Some((text, source, source_map, glyphs)),
                _ => None,
            })
            .expect("the chapter set its title");
        let (text, source, source_map, glyphs) = title;
        assert_eq!(
            text, "A VOYAGE TO LILLIPUT",
            "the title was not drawn in capitals"
        );
        assert_eq!(
            source, "A Voyage to Lilliput",
            "the title lost its manuscript"
        );
        assert_eq!(
            source_map.len(),
            text.len() + 1,
            "the map does not cover every byte boundary of what was drawn",
        );
        // Every glyph's range indexes what was drawn, and taken
        // through the map it reads back the manuscript entire.
        let read: String = glyphs
            .iter()
            .map(|glyph| {
                let from = source_map[glyph.range.start as usize] as usize;
                let to = source_map[glyph.range.end as usize] as usize;
                &source[from..to]
            })
            .collect();
        assert_eq!(read, *source, "the glyphs do not read the manuscript back");

        // A book nothing transformed says nothing twice over.
        let plain = paginate(vec![section(vec![heading("A Voyage to Lilliput")])]);
        assert!(
            plain[0].items.iter().all(|item| !matches!(
                item,
                DrawItem::Text { source, .. } if !source.is_empty()
            )),
            "an untransformed run has a source of its own",
        );
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

    /// Folios are correct and sequential — every page that shows a
    /// folio shows its own number, and the folios read in page order
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
        // Counted, not shown: the page after an open shows its own
        // number, one past the blind one.
        for open in opens {
            if let Some(next) = pages.get(open as usize) {
                assert_eq!(folio(next).map(|(_, d)| d), Some((open + 1).to_string()));
            }
        }
    }

    /// Every page names the section its content came out of, and the
    /// chapters read across the book in the order they were written.
    #[test]
    fn a_page_names_the_section_it_holds_content_from() {
        let book = book_of(vec![chapter("Chapter One", 14), chapter("Chapter Two", 14)]);
        let styles = crate::style::defaults(&book, registry());
        let pages = Paginator::new(registry(), &styles).paginate(&book);
        let ids: Vec<NodeId> = book.sections.iter().map(|s| s.id).collect();
        let mut read: Vec<NodeId> = Vec::new();
        for page in &pages {
            for id in &page.sections {
                if read.last() != Some(id) {
                    read.push(*id);
                }
            }
        }
        assert_eq!(read, ids, "the pages name the chapters out of order");
        // Each chapter opens where its own pages start.
        for (index, id) in ids.iter().enumerate() {
            let first = pages
                .iter()
                .position(|page| page.sections.contains(id))
                .expect("every chapter reaches a page");
            assert!(
                opens_a_chapter(&pages[first]),
                "chapter {index} first appears on a page that does not open one",
            );
        }
    }

    /// A leaf inserted to square the sheet has nobody's content on it,
    /// so it names no section.
    #[test]
    fn a_blank_leaf_names_no_section() {
        // A one-paragraph chapter between two long ones ends on its
        // own opening recto, which leaves a blank verso behind it.
        let pages = paginate(vec![
            chapter("Chapter One", 14),
            section(vec![heading("Chapter Two"), paragraph("A short chapter.")]),
            chapter("Chapter Three", 14),
        ]);
        let blanks: Vec<&Page> = pages.iter().filter(|page| page.items.is_empty()).collect();
        assert!(
            !blanks.is_empty(),
            "expected a blank leaf in {} pages",
            pages.len()
        );
        for blank in blanks {
            assert!(
                blank.sections.is_empty(),
                "page {} is blank and still names {:?}",
                blank.number,
                blank.sections,
            );
        }
    }

    /// A chapter that ends mid-page is followed on that page by the
    /// next one opening, and the page names both, in that order.
    #[test]
    fn a_page_shared_by_two_chapters_names_both() {
        let css = "section { break-before: auto }";
        let book = book_of(vec![chapter("Chapter One", 3), chapter("Chapter Two", 3)]);
        let styles =
            crate::style::Stylesheets::parse(&[crate::style::Source::author("test.css", css)])
                .compile(&book, registry());
        let pages = Paginator::new(registry(), &styles).paginate(&book);
        let ids: Vec<NodeId> = book.sections.iter().map(|s| s.id).collect();
        let shared = pages
            .iter()
            .find(|page| page.sections.len() > 1)
            .expect("two short chapters running on share a page");
        assert_eq!(shared.sections, ids);
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
                .expect("body pages have a folio")
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

    /// Prose whose paragraphs are each set in a token of their own,
    /// so a page's lines can be traced back to the paragraph they
    /// were broken from. Lengths vary so page breaks land in every
    /// position a paragraph has.
    fn tagged_prose(paragraphs: usize) -> Vec<Block> {
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
    fn tagged_lines(page: &Page) -> Vec<String> {
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

    /// No paragraph is left a single line at either end of a page:
    /// the run of lines a page sets of a paragraph it shares with
    /// its neighbour is at least `widows` at the top and `orphans` at
    /// the bottom.
    fn assert_orphans_and_widows(pages: &[Page], orphans: usize, widows: usize) {
        let tagged: Vec<Vec<String>> = pages.iter().map(tagged_lines).collect();
        assert_orphans_and_widows_over(&tagged, "page", orphans, widows);
    }

    /// The same over whatever the flow filled in order, which is
    /// pages on an undivided page box and columns on a divided one.
    fn assert_orphans_and_widows_over(
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

    /// Acceptance: no single line of a paragraph is stranded at a
    /// page boundary, either end — under the built-in sheet's two and
    /// two, and under an author's larger numbers.
    #[test]
    fn orphans_and_widows_hold_at_every_page_boundary() {
        assert_eq!((ua().root().orphans, ua().root().widows), (2, 2));
        assert_orphans_and_widows(&paginate(vec![section(tagged_prose(60))]), 2, 2);
        let pages = paginate_styled(
            "p { orphans: 4; widows: 3 }",
            vec![section(tagged_prose(60))],
        );
        assert_orphans_and_widows(&pages, 4, 3);
    }

    /// The indent one level of quotation adds under the built-in
    /// sheet.
    fn quote_indent() -> f32 {
        let book = book_of(vec![section(vec![quote(vec![paragraph("quoted")])])]);
        let styles = crate::style::defaults(&book, registry());
        let node = styles
            .nodes()
            .iter()
            .find(|node| node.element == "blockquote")
            .expect("the sample book has a blockquote");
        styles.styles()[node.style as usize].margin.left
    }

    /// Acceptance: a blockquote nested two deep indents twice, and
    /// splitting it across a page turn does not lose the indent.
    #[test]
    fn a_nested_blockquote_indents_twice_and_keeps_it_across_a_page() {
        let inner = vec!["inner"; 900].join(" ");
        let pages = paginate(vec![section(vec![
            paragraph(&"outside the quotation ".repeat(20)),
            quote(vec![
                paragraph(&"once removed ".repeat(20)),
                quote(vec![paragraph(&inner)]),
            ]),
        ])]);
        let indent = quote_indent();
        assert!(indent > 0.0, "the sheet indents nothing");

        let mut spanned = 0;
        for page in &pages {
            let (left, _) = origin_of(page);
            let mut seen = false;
            for (_, runs) in content_lines(page) {
                let (x, _, text) = runs[0];
                if !text.starts_with("inner") {
                    continue;
                }
                seen = true;
                assert!(
                    (x - left - 2.0 * indent).abs() < 1e-3,
                    "page {}: the nested quote sits at {}, not {}",
                    page.number,
                    x - left,
                    2.0 * indent,
                );
            }
            spanned += seen as usize;
        }
        assert!(
            spanned >= 2,
            "the nested quote fitted on {spanned} page(s); nothing was split",
        );

        // The measure narrows with the indent: every line of the
        // nested quote ends inside a box two indents narrower.
        let measure = master(Situation::Body(Side::Recto)).geometry.measure();
        for page in &pages {
            let (left, _) = origin_of(page);
            for item in &page.items {
                let DrawItem::Text { text, glyphs, .. } = item else {
                    continue;
                };
                if !text.starts_with("inner") {
                    continue;
                }
                for glyph in glyphs {
                    assert!(
                        glyph.x <= left + measure - indent,
                        "page {}: the nested quote runs past its measure",
                        page.number,
                    );
                }
            }
        }
    }

    /// `text-indent` sinks the first line of a paragraph and leaves
    /// the rest of it at the full measure.
    #[test]
    fn text_indent_moves_the_first_line_and_nothing_else() {
        let indent = 18.0;
        let pages = paginate_styled(
            &format!("p {{ text-indent: {indent}pt; text-align: left }}"),
            vec![section(vec![paragraph(&"a word ".repeat(60))])],
        );
        let page = pages.first().expect("the paragraph set no pages");
        let (left, _) = origin_of(page);
        let lines = content_lines(page);
        assert!(
            lines.len() > 2,
            "the paragraph broke into {} line(s)",
            lines.len()
        );

        let (_, first) = &lines[0];
        assert!(
            (first[0].0 - left - indent).abs() < 1e-3,
            "the first line starts at {}, not {indent} in",
            first[0].0 - left,
        );
        for (_, runs) in &lines[1..] {
            assert!(
                (runs[0].0 - left).abs() < 1e-3,
                "a later line starts at {}, not at the margin",
                runs[0].0 - left,
            );
        }

        // The indent is taken out of the measure rather than hung
        // past it: the first line still ends inside the content box.
        let measure = master(Situation::Body(page.side)).geometry.measure();
        for item in &page.items {
            let DrawItem::Text { glyphs, size, .. } = item else {
                continue;
            };
            if *size == folio_size() {
                continue;
            }
            for glyph in glyphs {
                assert!(
                    glyph.x <= left + measure,
                    "a glyph at {} runs past the measure",
                    glyph.x - left,
                );
            }
        }
    }

    /// The first-line indent the built-in sheet gives ordinary
    /// prose: a paragraph with another one above it.
    fn prose_indent() -> f32 {
        let book = book_of(vec![section(vec![
            paragraph("opening"),
            paragraph("following"),
        ])]);
        let styles = crate::style::defaults(&book, registry());
        let node = styles
            .nodes()
            .iter()
            .filter(|node| node.element == "p")
            .nth(1)
            .expect("the sample book has a second paragraph");
        styles.styles()[node.style as usize].text_indent
    }

    /// The computed indent of the `nth` paragraph of a book styled by
    /// the built-in sheet alone.
    fn indent_of(sections: Vec<Section>, nth: usize) -> f32 {
        let book = book_of(sections);
        let styles = crate::style::defaults(&book, registry());
        let node = styles
            .nodes()
            .iter()
            .filter(|node| node.element == "p")
            .nth(nth)
            .expect("the book has that many paragraphs");
        styles.styles()[node.style as usize].text_indent
    }

    /// Acceptance: the built-in sheet names the convention. A
    /// paragraph following another one indents; the paragraph a
    /// chapter opens with and the one a scene break starts again
    /// after do not.
    #[test]
    fn the_built_in_sheet_indents_prose_but_not_an_opening() {
        let indent = prose_indent();
        assert!(indent > 0.0, "the sheet indents nothing");

        // Read off the tree first: an opening paragraph is flush
        // whether a heading or nothing at all stands above it.
        let words = "my father had a small estate in nottinghamshire ";
        let opening = vec![section(vec![
            heading("Chapter One"),
            paragraph(&words.repeat(4)),
            paragraph(&words.repeat(4)),
        ])];
        assert_eq!(indent_of(opening.clone(), 0), 0.0);
        assert_eq!(indent_of(opening, 1), indent);
        assert_eq!(
            indent_of(vec![section(vec![paragraph("alone")])], 0),
            0.0,
            "a section opening on prose indented its first paragraph",
        );

        // And read it off the page: four paragraphs, the first under
        // a heading and the third after a scene break.
        let tagged = |tag: &str| paragraph(&format!("{tag} {}", words.repeat(3)));
        let pages = paginate(vec![section(vec![
            heading("Chapter One"),
            tagged("alpha"),
            tagged("bravo"),
            scene_break(),
            tagged("charlie"),
            tagged("delta"),
        ])]);
        let page = pages.first().expect("the chapter set no pages");
        let (left, _) = origin_of(page);
        for (tag, expected) in [
            ("alpha", 0.0),
            ("bravo", indent),
            ("charlie", 0.0),
            ("delta", indent),
        ] {
            let (_, runs) = content_lines(page)
                .into_iter()
                .find(|(_, runs)| runs[0].2.starts_with(tag))
                .unwrap_or_else(|| panic!("no line opens with {tag}"));
            assert!(
                (runs[0].0 - left - expected).abs() < 1e-3,
                "{tag} starts {}pt in, not {expected}pt",
                runs[0].0 - left,
            );
        }
    }

    /// Acceptance: justification resolves against the shortened
    /// first-line measure, so an indented first line still ends
    /// flush on the measure's right edge.
    #[test]
    fn a_justified_first_line_still_ends_on_the_measure() {
        let indent = 24.0;
        let pages = paginate_styled(
            &format!("p {{ text-indent: {indent}pt; text-align: justify }}"),
            vec![section(vec![paragraph(
                &"my father had a small estate ".repeat(20),
            )])],
        );
        let page = pages.first().expect("the paragraph set no pages");
        let (left, _) = origin_of(page);
        let measure = master(Situation::Body(page.side)).geometry.measure();
        let lines = content_lines(page);
        assert!(lines.len() > 2, "not enough lines to justify");

        // Every line but the last reaches the right edge, the
        // indented first one included: its own edge is the same edge.
        for (index, (baseline, _)) in lines.iter().enumerate().take(lines.len() - 1) {
            let right = right_edge(page, *baseline);
            assert!(
                (right - left - measure).abs() < 0.5,
                "line {index} ends {}pt in, not on the {measure}pt measure",
                right - left,
            );
        }
    }

    /// Acceptance: a paragraph split over a page turn indents its
    /// first line and nothing else. The continuation opens flush at
    /// the top of the next page.
    #[test]
    fn a_paragraph_broken_across_a_page_indents_once() {
        let indent = 18.0;
        let pages = paginate_styled(
            &format!("p {{ text-indent: {indent}pt; text-align: left }}"),
            vec![section(vec![paragraph(
                &"my father had a small estate in nottinghamshire ".repeat(220),
            )])],
        );
        assert!(pages.len() > 1, "the paragraph fitted on one page");
        let mut indented = 0;
        for page in &pages {
            let (left, _) = origin_of(page);
            for (index, (_, runs)) in content_lines(page).iter().enumerate() {
                let start = runs[0].0 - left;
                if (start - indent).abs() < 1e-3 {
                    assert_eq!(
                        (page.number, index),
                        (pages[0].number, 0),
                        "page {} indented line {index}",
                        page.number,
                    );
                    indented += 1;
                } else {
                    assert!(
                        start.abs() < 1e-3,
                        "page {}: line {index} starts {start}pt in",
                        page.number,
                    );
                }
            }
        }
        assert_eq!(indented, 1, "the paragraph indented {indented} lines");
    }

    /// Acceptance: a drop cap and an indent do not stack. The cap's
    /// reserved measure is what offsets its line; the indent the
    /// sheet asks for adds nothing on top of it.
    #[test]
    fn a_drop_cap_absorbs_the_indent() {
        let prose = "my father had a small estate in nottinghamshire ".repeat(12);
        let capped = |css: &str| {
            let pages = paginate_styled(css, vec![section(vec![paragraph(&prose)])]);
            let page = pages.first().expect("the paragraph set no pages");
            let (left, _) = origin_of(page);
            content_lines(page)
                .iter()
                .map(|(_, runs)| runs[0].0 - left)
                .collect::<Vec<f32>>()
        };
        let plain = capped("p::first-letter { initial-letter: 3 }");
        let indented = capped("p::first-letter { initial-letter: 3 } p { text-indent: 18pt }");
        assert!(plain.len() > 4, "not enough lines to sink into");
        assert_eq!(
            plain, indented,
            "the indent moved a line the cap had already displaced",
        );
    }

    /// Acceptance: a quotation indents from its own leading edge,
    /// not the page's.
    #[test]
    fn a_quotes_indent_starts_at_its_own_edge() {
        let indent = 9.0;
        let margin = quote_indent();
        let pages = paginate_styled(
            &format!("blockquote p {{ text-indent: {indent}pt; text-align: left }}"),
            vec![section(vec![quote(vec![paragraph(
                &"quoted prose runs on for a while ".repeat(12),
            )])])],
        );
        let page = pages.first().expect("the quote set no pages");
        let (left, _) = origin_of(page);
        let lines = content_lines(page);
        assert!(
            lines.len() > 2,
            "the quote broke into {} line(s)",
            lines.len()
        );
        assert!(
            (lines[0].1[0].0 - left - margin - indent).abs() < 1e-3,
            "the first line starts {}pt in, not {}pt",
            lines[0].1[0].0 - left,
            margin + indent,
        );
        for (_, runs) in &lines[1..] {
            assert!(
                (runs[0].0 - left - margin).abs() < 1e-3,
                "a later line starts {}pt in, not at the quote's edge",
                runs[0].0 - left,
            );
        }
    }

    /// The ornament the built-in sheet sets a thematic break in.
    fn ornament() -> String {
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

    /// Acceptance: a scene break paints between the paragraphs it
    /// separates, centred in the measure, and never lands alone at a
    /// page boundary — neither closing a page nor opening one.
    #[test]
    fn a_scene_break_paints_between_paragraphs_and_never_lands_alone() {
        let mark = ornament();
        let words = "the drawer of knives was where it had always been and yet ";
        let mut blocks = Vec::new();
        for index in 0..24 {
            if index > 0 {
                blocks.push(scene_break());
            }
            blocks.push(paragraph(&words.repeat(3 + index % 4)));
        }
        let pages = paginate(vec![section(blocks)]);

        let mut painted = 0;
        let measure = master(Situation::Body(Side::Recto)).geometry.measure();
        for page in &pages {
            let lines = content_lines(page);
            let marks: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, (_, runs))| runs[0].2 == mark)
                .map(|(index, _)| index)
                .collect();
            painted += marks.len();
            for index in marks {
                assert!(
                    index > 0,
                    "page {}: a scene break opened the page",
                    page.number,
                );
                assert!(
                    index + 1 < lines.len(),
                    "page {}: a scene break closed the page",
                    page.number,
                );
                let (left, _) = origin_of(page);
                let (x, size, text) = lines[index].1[0];
                let width = registry()
                    .shape(ua().root().font_id, text)
                    .unwrap_or_default()
                    .iter()
                    .map(|glyph| glyph.x_advance as f32)
                    .sum::<f32>()
                    / registry()
                        .metrics(ua().root().font_id)
                        .unwrap()
                        .units_per_em as f32
                    * size;
                assert!(
                    (x + width / 2.0 - left - measure / 2.0).abs() < 1e-3,
                    "page {}: the ornament is not centred in the measure",
                    page.number,
                );
            }
        }
        assert_eq!(painted, 23, "every scene break paints exactly once");
    }

    /// The text runs of one page, grouped by the baseline they share,
    /// each with whether it was shaped in small capitals.
    fn small_caps_lines(page: &Page) -> Vec<(f32, Vec<(&str, bool)>)> {
        let mut lines: Vec<(f32, Vec<(&str, bool)>)> = Vec::new();
        for item in &page.items {
            let DrawItem::Text {
                y,
                size,
                text,
                features,
                ..
            } = item
            else {
                continue;
            };
            if *size == folio_size() {
                continue;
            }
            let run = (text.as_str(), features.small_caps);
            match lines.last_mut() {
                Some((baseline, runs)) if (*baseline - y).abs() < 1e-3 => runs.push(run),
                _ => lines.push((*y, vec![run])),
            }
        }
        lines
    }

    /// A section of an `h3` and one long paragraph, which is what a
    /// chapter opening styled through `h3 + p` needs.
    fn under_h3(prose: &str) -> Vec<Section> {
        vec![section(vec![
            Block::Heading {
                id: NodeId::UNASSIGNED,
                level: HeadingLevel::H3,
                inlines: vec![text("A Voyage")],
                attributes: Attributes::default(),
                position: None,
                span: None,
            },
            paragraph(prose),
        ])]
    }

    /// Acceptance: `h3 + p::first-line { font-variant-caps:
    /// small-caps }` draws the opening line in small capitals and the
    /// rest of the paragraph in the letters the author wrote. The
    /// change stops at the break the paragraph came to: the last run
    /// of the first line is small capitals and the first run of the
    /// second is not.
    #[test]
    fn a_first_line_of_small_capitals_stops_where_the_line_does() {
        let prose = "my father had a small estate in nottinghamshire ".repeat(6);
        let pages = paginate_styled(
            "h3 + p::first-line { font-variant-caps: small-caps }",
            under_h3(&prose),
        );
        let lines = small_caps_lines(&pages[0]);
        assert!(lines.len() > 3, "not enough lines to break");

        // The heading is the first baseline; the paragraph follows.
        let opening = &lines[1].1;
        let next = &lines[2].1;
        assert!(
            opening.iter().all(|(_, small)| *small),
            "the opening line is not all small capitals: {opening:?}",
        );
        assert!(
            next.iter().all(|(_, small)| !*small),
            "the small capitals ran past the first line: {next:?}",
        );
        assert!(
            lines[3..]
                .iter()
                .all(|(_, runs)| runs.iter().all(|(_, small)| !*small)),
            "the small capitals reached further down the page",
        );

        // The letters are the ones the author wrote: the face draws
        // the capitals, the text does not spell them.
        // A break swallows the space it falls on, so the lines join
        // back with one between them.
        let set = lines[1..]
            .iter()
            .map(|(_, runs)| runs.iter().map(|(text, _)| *text).collect::<String>())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            prose.starts_with(set.trim_end()),
            "the paragraph was not set as it was written: {set:?}",
        );
    }

    /// Acceptance: a drop cap and a small-capitals first line set
    /// together. Both fall on the initial and `::first-letter` wins
    /// it: the cap is the one run set larger and it is not small
    /// capitals, while the line beside it is.
    #[test]
    fn a_drop_cap_takes_the_initial_from_the_first_line() {
        let prose = "my father had a small estate in nottinghamshire ".repeat(6);
        let pages = paginate_styled(
            "h3 + p::first-letter { initial-letter: 3 }
             h3 + p::first-line { font-variant-caps: small-caps }",
            under_h3(&prose),
        );
        let lines = small_caps_lines(&pages[0]);
        // The cap is the one run of the paragraph set larger than the
        // body; the heading above it is larger too.
        let cap = pages[0]
            .items
            .iter()
            .filter_map(|item| match item {
                DrawItem::Text {
                    size,
                    text,
                    features,
                    ..
                } if *size > 1.5 * body_size() && text != "A Voyage" => {
                    Some((text.as_str(), features.small_caps))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            cap,
            vec![("m", false)],
            "the initial is not one run of its own, out of the first line's small capitals",
        );

        let opening: Vec<(&str, bool)> = lines[1]
            .1
            .iter()
            .copied()
            .filter(|(text, _)| *text != "m")
            .collect();
        assert!(
            !opening.is_empty() && opening.iter().all(|(_, small)| *small),
            "the line beside the cap is not small capitals: {opening:?}",
        );
    }

    /// Acceptance: a three-line drop cap sits on the third baseline,
    /// its top on the first line's cap height, and the lines beside
    /// it are set to the measure it left them.
    #[test]
    fn a_drop_cap_aligns_to_the_third_baseline_and_shortens_three_lines() {
        let pages = paginate_styled(
            "p::first-letter { initial-letter: 3 }",
            vec![section(vec![paragraph(
                &"my father had a small estate in nottinghamshire ".repeat(12),
            )])],
        );
        let lines = content_lines(&pages[0]);
        assert!(lines.len() > 4, "not enough lines to sink into");

        let body = ua().root();
        let (left, _) = origin_of(&pages[0]);
        // The cap is the one run set larger than the body.
        let (cap_index, cap) = lines
            .iter()
            .enumerate()
            .find(|(_, (_, runs))| runs[0].1 > body.font_size)
            .map(|(index, (baseline, runs))| (index, (*baseline, runs[0])))
            .expect("a drop cap paints");
        let (cap_baseline, (cap_x, cap_size, _)) = cap;

        // Its baseline is the third line's, and it starts at the
        // content box's own leading edge.
        let prose: Vec<&ContentLine<'_>> = lines
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != cap_index)
            .map(|(_, line)| line)
            .collect();
        assert!(
            (cap_baseline - prose[2].0).abs() < 1e-3,
            "the cap is not on the third baseline"
        );
        assert!(
            (cap_x - left).abs() < 1e-3,
            "the cap is not at the leading edge"
        );

        // Its top sits on the first line's cap height.
        let cap_height = |font: u16, size: f32| {
            let metrics = registry().metrics(font).unwrap();
            metrics.cap_height as f32 / metrics.units_per_em as f32 * size
        };
        let top = cap_baseline - cap_height(body.font_id, cap_size);
        assert!(
            (top - (prose[0].0 - cap_height(body.font_id, body.font_size))).abs() < 1e-2,
            "the cap's top is not the first line's cap height",
        );

        // The three lines beside it start past the cap and are set to
        // the measure it left them; the fourth is back at the full one.
        let sunk = prose[0].1[0].0;
        assert!(sunk > left, "the first line was not moved aside");
        for line in prose.iter().take(3) {
            assert!(
                (line.1[0].0 - sunk).abs() < 1e-3,
                "a sunk line is not set to the shortened measure",
            );
        }
        assert!(
            (prose[3].1[0].0 - left).abs() < 1e-3,
            "the fourth line did not go back to the full measure",
        );
        for page in &pages {
            for item in &page.items {
                let DrawItem::Text { glyphs, .. } = item else {
                    continue;
                };
                for glyph in glyphs {
                    assert!(
                        glyph.x <= left + master(Situation::Body(page.side)).geometry.measure(),
                        "a sunk line ran past the measure",
                    );
                }
            }
        }
    }

    /// A PNG header of the given pixel size, at the default 96dpi.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(13u32.to_be_bytes());
        bytes.extend(b"IHDR");
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    /// Acceptance: an image taller than the content box is scaled to
    /// fit it, keeping its ratio, and the run says so.
    #[test]
    fn an_image_taller_than_the_content_box_scales_and_warns() {
        struct Png;
        impl crate::images::ImageLoader for Png {
            fn load(&self, url: &str) -> Option<Vec<u8>> {
                match url {
                    // 8in by 16in at 96dpi: taller than the page.
                    "tall.png" => Some(png(768, 1536)),
                    // 2in square: it fits as it is.
                    "small.png" => Some(png(192, 192)),
                    _ => None,
                }
            }
        }

        let image = |url: &str| Block::Image {
            id: NodeId::UNASSIGNED,
            url: url.into(),
            alt: "a drawer of knives".into(),
            attributes: Attributes::default(),
            position: Some(SourcePos { line: 9, column: 1 }),
            span: None,
        };
        let book = book_of(vec![section(vec![
            paragraph("before"),
            image("tall.png"),
            image("small.png"),
        ])]);
        let styles = crate::style::defaults(&book, registry());
        let assets = crate::images::Assets::probe(&book, &Png);
        let output = layout_book(&book, &styles, registry(), &assets);

        let placed: Vec<(f32, f32, u32)> = output
            .pages
            .iter()
            .flat_map(|page| page.items.iter())
            .filter_map(|item| match item {
                DrawItem::Image { w, h, asset, .. } => Some((*w, *h, *asset)),
                _ => None,
            })
            .collect();
        assert_eq!(placed.len(), 2, "both images are placed");

        let (_, height) = master(Situation::Body(Side::Recto)).geometry.content_size();
        let (width, tall, asset) = placed[0];
        assert_eq!(asset, 0, "the first image indexes the first asset");
        assert!(
            (tall - height).abs() < 1e-3,
            "the tall image is {tall}pt in a {height}pt box",
        );
        assert!(
            (width / tall - 0.5).abs() < 1e-3,
            "scaling did not keep the ratio: {width} by {tall}",
        );
        // The one that fits keeps its intrinsic size: 2in square.
        assert_eq!(placed[1], (144.0, 144.0, 1));

        let warning = output
            .warnings
            .iter()
            .find(|warning| warning.message.contains("tall.png"))
            .expect("scaling an image to fit is worth saying");
        assert!(warning.message.contains("taller than the content box"));
        assert_eq!(warning.origin.as_deref(), Some("9:1"));
        assert!(
            !output
                .warnings
                .iter()
                .any(|w| w.message.contains("small.png")),
            "an image that fits is not worth a diagnostic",
        );
    }

    /// `break-before` and `break-after` reach fragmentation from the
    /// cascade, and nothing in the paginator hardcodes them: a sheet
    /// that turns the recto rule off runs the chapters together, and
    /// one that asks for a page break gets one.
    #[test]
    fn break_control_comes_from_the_cascade() {
        let chapters = || {
            vec![
                section(vec![heading("One"), paragraph("The first chapter.")]),
                section(vec![heading("Two"), paragraph("The second chapter.")]),
            ]
        };
        // The built-in sheet opens a chapter on a recto.
        assert_eq!(paginate(chapters()).len(), 3);
        // The author turns that off and the chapters run together.
        assert_eq!(
            paginate_styled("section { break-before: auto }", chapters()).len(),
            1,
        );
        // A page break, without a side, is still a page break.
        assert_eq!(
            paginate_styled("section { break-before: page }", chapters()).len(),
            2,
        );
        // And a verso open leaves the blank recto behind it.
        let pages = paginate_styled("section { break-before: verso }", chapters());
        assert_eq!(pages.len(), 4);
        assert!(pages[0].items.is_empty() || pages[2].items.is_empty());
    }

    /// `break-inside: avoid` moves a block whole rather than split
    /// it, and `break-after: avoid` keeps a heading with the prose
    /// under it.
    #[test]
    fn avoid_keeps_blocks_and_headings_with_what_follows_them() {
        // A page of single-line paragraphs, all but a few lines
        // full, and then a quotation too long for what is left.
        let quoted = "quoted words that would rather not be split across a page turn ";
        let filler: Vec<Block> = (0..30)
            .map(|index| paragraph(&format!("filler line {index}")))
            .collect();
        let blocks = [
            filler,
            vec![quote(vec![paragraph(&quoted.repeat(6))])],
            long_prose(2),
        ]
        .concat();
        // A quoted line is one set at the quotation's indent: only
        // its first begins with the words the quotation opens with.
        let indent = quote_indent();
        let split = |css: &str| {
            let pages = paginate_styled(css, vec![section(blocks.clone())]);
            pages
                .iter()
                .filter(|page| {
                    let (left, _) = origin_of(page);
                    content_lines(page)
                        .iter()
                        .any(|(_, runs)| (runs[0].0 - left - indent).abs() < 1e-3)
                })
                .count()
        };
        assert!(split("") >= 2, "the quotation should straddle a page");
        assert_eq!(
            split("blockquote { break-inside: avoid }"),
            1,
            "an avoided blockquote should move whole",
        );

        // A heading is never the last thing on a page: the built-in
        // sheet gives it `break-after: avoid`.
        let pages = paginate(vec![
            section(long_prose(12)),
            section([vec![heading("Two")], long_prose(12)].concat()),
        ]);
        for page in &pages {
            let lines = content_lines(page);
            if let Some((_, runs)) = lines.last() {
                assert!(
                    runs[0].1 != chapter_size(),
                    "page {}: a heading closed the page",
                    page.number,
                );
            }
        }
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

        let staged: Vec<Vec<Fragment>> = book
            .sections
            .iter()
            .map(|section| paginator.section_fragments(section))
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
}
