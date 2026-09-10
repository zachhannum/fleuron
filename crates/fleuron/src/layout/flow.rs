//! Fragments in, pages out: what stacks in a column, where a
//! column ends, and what a page paints when it closes.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::content::{NodeId, Section};
use crate::pages::{DrawItem, Page, Side};
use crate::style::{Break, PageQuery, Situation};

use super::Paginator;
use super::build::Reflow;
use super::exclusion::AnchoredImages;
use super::fragment::{BreakPoint, Decoration, Decorations, Fragment, Marks, Piece};
use super::furniture::Strings;

/// What one page needs to know to ask the style tree for its master:
/// the named page in force, and the situation the page is in.
#[derive(Debug, Clone)]
pub(super) struct PageSlot {
    name: Option<String>,
    /// The page a section opens on: `@page :first`.
    first: bool,
    /// Inserted to square the sheet: `@page :blank`.
    pub(super) blank: bool,
}

impl PageSlot {
    pub(super) fn query(&self, side: Side) -> PageQuery<'_> {
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

/// What is recorded about one finished page: the master to ask for,
/// the running strings as they stood when it opened, and the folio it
/// restarts at.
pub(crate) struct PageInfo {
    pub(super) slot: PageSlot,
    pub(super) strings: Strings,
    pub(super) reset: Option<u32>,
    /// Items the flow itself painted, before any furniture.
    pub(super) content_items: usize,
}

/// Pages as fragmentation settled them, and what each one needs to
/// paint its furniture. The two travel together, because a folio is a
/// fact about where a page landed rather than about what is on it.
pub(crate) struct Paged {
    pub(crate) pages: Vec<Page>,
    pub(crate) infos: Vec<PageInfo>,
    /// The page each anchor landed on, by the node it was written at.
    pub(crate) anchors: BTreeMap<NodeId, usize>,
}

/// One fragment placed on the page being built.
pub(super) struct Placed {
    /// The section its content came out of. The fragment records it, so
    /// a fragment moved onto the next page counts toward the page it
    /// ends on rather than the one it was measured for.
    section: NodeId,
    /// The column of the page it landed in.
    column: u32,
    /// Top of its box, from its column's top.
    pub(super) top: f32,
    /// Its own height.
    pub(super) height: f32,
    /// Whether a page may end above it.
    break_before: BreakPoint,
    /// What it paints, already positioned on this page.
    items: Vec<DrawItem>,
    /// What it sets for the furniture of whichever page it ends on.
    marks: Option<Box<Marks>>,
    /// The decorated blocks it opens and closes.
    decorations: Option<Box<Decorations>>,
    /// The images anchored above it, which land on the page it ends
    /// on.
    pub(super) anchors: Vec<NodeId>,
}

/// The flow: fragments in, pages out.
///
/// The page being built is a list of placed fragments rather than a
/// finished structure, because a fragment that does not fit can
/// push the ones above it onto the next page — moving what is already
/// painted, never measuring it again.
pub(super) struct Flow<'a, 'p> {
    pub(super) paginator: &'p Paginator<'a>,
    pub(super) pages: Vec<Page>,
    infos: Vec<PageInfo>,
    pub(super) placed: Vec<Placed>,
    pub(super) slot: PageSlot,
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
    pub(super) cursor: f32,
    /// Height of the column being filled, which is the content box's.
    pub(super) height: f32,
    /// The column being filled, counting from the leading edge.
    pub(super) column: u32,
    /// How many the page divides into.
    columns: u32,
    /// Where in `placed` the column being filled began. A break backs
    /// up to a fragment of this column, never past its head.
    pub(super) column_start: usize,
    /// The decorated blocks the page being built opened with,
    /// outermost first: a block the page before it did not finish.
    carried: Vec<Decoration>,
    /// The images to place, and the page each one landed on. Empty
    /// on the pass that answers where they land.
    pub(super) anchored: &'p AnchoredImages,
    /// Where each anchor landed, filled in as pages close.
    anchors: BTreeMap<NodeId, usize>,
    /// Anchors waiting for the fragment whose page they take.
    pub(super) pending_anchors: Vec<NodeId>,
    /// Whether what is placed is painted. The pass that settles where
    /// the anchors land keeps no pages, so it paints nothing: which
    /// page a fragment falls on is a question about heights.
    paints: bool,
}

impl<'a, 'p> Flow<'a, 'p> {
    pub(super) fn new(paginator: &'p Paginator<'a>, anchored: &'p AnchoredImages) -> Flow<'a, 'p> {
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
            anchored,
            anchors: BTreeMap::new(),
            pending_anchors: Vec::new(),
            paints: true,
        }
    }

    /// A flow that answers where the anchors land and nothing else.
    pub(super) fn settling(paginator: &'p Paginator<'a>, bare: &'p AnchoredImages) -> Flow<'a, 'p> {
        Flow {
            paints: false,
            ..Flow::new(paginator, bare)
        }
    }

    /// Flows one section. Its page name and `@page :first` master are
    /// claimed by the page it opens — when it opens one at all: a
    /// section that breaks `auto` continues where the last left off.
    pub(super) fn section(&mut self, section: &Section, fragments: &[Fragment]) {
        let style = self.paginator.styles.style(section.id);
        self.section = section.id;
        self.pending_slot = Some(PageSlot {
            name: style.page.clone(),
            first: true,
            blank: false,
        });
        // A book with nothing anchored places one fragment at a time.
        // One with an image on the page places a paragraph at a time,
        // because an image narrows the bands the paragraph is set in.
        // The whole of it is then broken again.
        let mut index = 0;
        while index < fragments.len() {
            index = match fragments[index].reflow.as_ref() {
                Some(reflow) if !self.anchored.is_empty() => {
                    let end = paragraph_end(fragments, index, reflow);
                    self.paragraph(&fragments[index..end], reflow);
                    end
                }
                _ => {
                    self.place(&fragments[index]);
                    index + 1
                }
            };
        }
    }

    /// Places one fragment, ending columns and pages as its break
    /// point demands.
    pub(super) fn place(&mut self, fragment: &Fragment) {
        // An anchor is not placed. It binds to the next fragment that
        // is, and takes the page that fragment ends on.
        if let Piece::Anchor(node) = fragment.piece {
            self.pending_anchors.push(node);
            return;
        }
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
        // waiting for is this one. Either way the section had its
        // chance to claim one.
        if self.placed.is_empty()
            && let Some(slot) = self.pending_slot.take()
        {
            self.slot = slot;
            self.remaster();
        }
        self.pending_slot = None;
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

    /// Paints one fragment onto the page being built.
    fn emit(&mut self, fragment: &Fragment, lead: f32) {
        let (x, y) = self.origin();
        let top = self.cursor + lead + fragment.fixed;
        let items = match &fragment.piece {
            _ if !self.paints => Vec::new(),
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
            Piece::Blank | Piece::Anchor(_) => Vec::new(),
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
            anchors: std::mem::take(&mut self.pending_anchors),
        });
    }

    /// Whether nothing stands in the column being filled.
    pub(super) fn column_empty(&self) -> bool {
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
        // Backgrounds, borders, column rules and images go in front
        // of the page's text: `DrawItem` order is paint order, and
        // the display structure has no layers.
        let mut items = Vec::new();
        if self.paints {
            items = self.decorate(&placed);
            items.append(&mut self.rules(&placed));
            items.append(&mut self.anchored_items());
        }
        let index = self.pages.len();
        let mut sections: Vec<NodeId> = Vec::new();
        for placed in placed {
            if sections.last() != Some(&placed.section) {
                sections.push(placed.section);
            }
            for node in placed.anchors {
                self.anchors.insert(node, index);
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

    fn remaster(&mut self) {
        let geometry = self.paginator.master(self.pages.len(), &self.slot).geometry;
        self.height = geometry.content_size().1;
        self.columns = geometry.column_count();
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

    /// The images the page being built carries, as paint ops.
    fn anchored_items(&self) -> Vec<DrawItem> {
        let index = self.pages.len();
        let Some(anchored) = self.anchored.by_page.get(&index) else {
            return Vec::new();
        };
        let geometry = self.paginator.master(index, &self.slot).geometry;
        anchored
            .iter()
            .map(|at| self.anchored.all[*at].item(geometry))
            .collect()
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

    pub(super) fn finish(mut self) -> Paged {
        self.close();
        // An anchor with nothing after it lands on the last page the
        // book reached.
        let last = self.pages.len().saturating_sub(1);
        for node in std::mem::take(&mut self.pending_anchors) {
            self.anchors.insert(node, last);
        }
        Paged {
            pages: self.pages,
            infos: self.infos,
            anchors: self.anchors,
        }
    }
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

/// Where one paragraph's fragments end: the run of them that share
/// the paragraph `from` opens.
fn paragraph_end(fragments: &[Fragment], from: usize, reflow: &Arc<Reflow>) -> usize {
    fragments[from..]
        .iter()
        .position(|fragment| {
            !fragment
                .reflow
                .as_ref()
                .is_some_and(|other| Arc::ptr_eq(other, reflow))
        })
        .map(|at| from + at)
        .unwrap_or(fragments.len())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Block, NodeId};
    use crate::layout::testing::{
        Run, assert_orphans_and_widows_over, body_size, book_of, chapter, chapter_size,
        content_items, content_lines, folio_size, heading, long_prose, master, opens_a_chapter,
        page_geometry, paginate, paginate_styled, paragraph, prose, quote, rects, registry,
        scene_break, section, styled_geometry, tagged_prose, ua,
    };
    use crate::pages::{DrawItem, Page, Side};
    use crate::style::{Color, Situation};

    /// A page divided in two, with a gutter wide enough to tell the
    /// columns apart by where a line starts.
    const TWO_COLUMNS: &str = "@page { column-count: 2; column-gap: 18pt }";

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

    /// A book with no content produces no pages.
    #[test]
    fn empty_book_yields_no_pages() {
        assert!(paginate(vec![]).is_empty());
        assert!(paginate(vec![section(vec![])]).is_empty());
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
}
