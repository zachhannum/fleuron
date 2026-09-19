//! Footnotes: what each note is numbered, what its body comes to,
//! and the area at the foot of the page its body is set in.
//!
//! A note is written in the flow and set under the column its
//! reference lands in. The area grows as the column takes notes, and
//! the room the column has for lines shrinks with it, so a line that
//! no longer fits moves on and takes its note with it.
//!
//! A note whose body outruns the room the column has left is split.
//! The rest of it opens the area of the next column, above the notes
//! that column takes for itself, and carries no reference of its own.
//!
//! A page that does not divide has one column, so its area is the
//! width of its content box.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::content::{Block, Book, Inline, NodeId, block_id, cell_blocks, notes_in_inlines};
use crate::pages::{DrawItem, PageBox};
use crate::style::{ComputedStyle, StyleTree};

use super::Paginator;
use super::build::{Builder, children, decorate};
use super::flow::{Flow, Placed};
use super::fragment::{Fragment, Marker};

/// What a book whose notes are numbered by page says when the
/// numbering does not settle.
pub(crate) const UNSETTLED: &str = "The notes are numbered by page and the numbering did not \
                                    settle. The numbers are the ones of the last layout.";

/// One note the flow sets at the foot of a page: what it is
/// numbered, and its body as fragments of the footnote area.
///
/// The lines that hold its reference carry it, so a line moved to
/// the next page moves the note with it.
#[derive(Debug)]
pub struct Note {
    /// The note element.
    pub node: NodeId,
    /// The number its reference prints.
    pub number: u32,
    /// Its body, broken to the measure of the area.
    pub fragments: Vec<Fragment>,
}

impl Note {
    /// The height of the fragments from `from`, the space above each
    /// one included. `under` is whether anything stands above the
    /// note in the area: the note that opens one drops the space
    /// above it.
    pub(super) fn height(&self, from: usize, under: bool) -> f32 {
        (from..self.fragments.len())
            .map(|index| self.step(index, under || index > from))
            .sum()
    }

    /// How far one fragment of the body takes the area down: its own
    /// height, and the space above it where anything is set above it.
    pub(super) fn step(&self, index: usize, under: bool) -> f32 {
        let fragment = &self.fragments[index];
        let lead = if under { fragment.lead } else { 0.0 };
        lead + fragment.fixed + fragment.height
    }
}

/// What the notes of a book are numbered.
///
/// The counter runs through the book, and `counter-reset: note`
/// restarts it: on a section, every chapter starts again; on the
/// footnote area, every page does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Numbering {
    numbers: BTreeMap<NodeId, u32>,
}

impl Numbering {
    /// The numbers the notes of `book` take, read in document order
    /// with the restarts the cascade asks for.
    pub(crate) fn of(book: &Book, styles: &StyleTree) -> Numbering {
        let mut numbering = Numbering::default();
        let mut next = 1;
        for section in &book.sections {
            numbering.restart(&mut next, styles.style(section.id));
            numbering.blocks(&section.blocks, styles, &mut next);
        }
        numbering
    }

    /// The same numbers, restarted on every page: `pages` says which
    /// page each note was set on, and `start` is the number the first
    /// note on a page takes.
    pub(crate) fn on_pages(&self, pages: &BTreeMap<NodeId, u32>, start: u32) -> Numbering {
        let mut numbers = BTreeMap::new();
        let mut page = None;
        let mut next = start;
        // The map is in node order, which is the order the notes were
        // written, and a page takes its notes in that order too.
        for (node, at) in pages {
            if page != Some(*at) {
                page = Some(*at);
                next = start;
            }
            numbers.insert(*node, next);
            next += 1;
        }
        Numbering { numbers }
    }

    /// What one note's reference prints. A note the numbering has
    /// never seen is the first of its own.
    pub(crate) fn number(&self, node: NodeId) -> u32 {
        self.numbers.get(&node).copied().unwrap_or(1)
    }

    fn blocks(&mut self, blocks: &[Block], styles: &StyleTree, next: &mut u32) {
        for block in blocks {
            self.restart(next, styles.style(block_id(block)));
            match block {
                Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                    self.inlines(inlines, styles, next)
                }
                Block::Blockquote { blocks, .. } => self.blocks(blocks, styles, next),
                Block::List { items, .. } => {
                    for item in items {
                        self.restart(next, styles.style(item.id));
                        self.blocks(&item.blocks, styles, next);
                    }
                }
                Block::Table { head, body, .. } => {
                    for blocks in cell_blocks(head, body) {
                        self.blocks(blocks, styles, next);
                    }
                }
                Block::CodeBlock { .. }
                | Block::ThematicBreak { .. }
                | Block::PageBreak { .. }
                | Block::ColumnBreak { .. }
                | Block::Image { .. } => {}
            }
        }
    }

    /// The same over the notes written among one block's inlines. A
    /// note written inside another takes the number after it.
    fn inlines(&mut self, inlines: &[Inline], styles: &StyleTree, next: &mut u32) {
        for note in notes_in_inlines(inlines) {
            let Inline::Note { id, blocks, .. } = note else {
                continue;
            };
            self.numbers.insert(*id, *next);
            *next += 1;
            self.blocks(blocks, styles, next);
        }
    }

    fn restart(&mut self, next: &mut u32, style: &ComputedStyle) {
        if let Some(number) = style.note_reset {
            *next = number;
        }
    }
}

impl StyleTree {
    /// Whether the notes of the book are numbered by the page they
    /// are set on, which the footnote area asks for by restarting the
    /// counter on itself.
    pub(crate) fn numbers_notes_per_page(&self) -> bool {
        self.style(self.notes_area()).note_reset.is_some()
    }

    /// The number the first note of a page takes.
    pub(crate) fn first_note_number(&self) -> u32 {
        self.style(self.notes_area()).note_reset.unwrap_or(1)
    }
}

impl Paginator<'_> {
    /// What one note's reference prints.
    pub(super) fn note_number(&self, node: NodeId) -> u32 {
        self.notes.borrow().number(node)
    }

    /// One note as the flow can set it: its body broken to the
    /// measure of the footnote area, with its number hanging before
    /// its first line.
    pub(super) fn note(&self, note: &Inline, source: Option<&str>) -> Option<Arc<Note>> {
        let Inline::Note {
            id,
            blocks,
            position,
            ..
        } = note
        else {
            return None;
        };
        let number = self.note_number(*id);
        let (x, measure) = self.note_measure();
        let style = self.styles.style(*id).clone();
        let mut builder = Builder::new(self, source);
        let start = builder.open(*id, &style, &[], x, measure);
        let (inner, narrowed) = style.content_box(x, measure);
        builder.blocks(
            children(self.styles, *id, blocks, *position),
            inner,
            narrowed,
        );
        if let Some(marker) = self.note_marker(&style, number, x, measure) {
            builder.hang(start, marker);
        }
        builder.close(&style, start);
        if builder
            .fragments
            .iter()
            .any(|fragment| fragment.notes.is_some())
        {
            self.warn(
                "A note was written inside another note. The note inside is left out.".to_string(),
                None,
            );
        }
        Some(Arc::new(Note {
            node: *id,
            number,
            fragments: builder.fragments,
        }))
    }

    /// Where the notes of a column start, and how wide they are set.
    pub(super) fn note_measure(&self) -> (f32, f32) {
        self.area_style().content_box(0.0, self.note_width())
    }

    /// How wide the area is: the measure of a column, which is the
    /// content box of a page that divides into one.
    pub(super) fn note_width(&self) -> f32 {
        self.styles.default_page().geometry.measure()
    }

    /// The number the note is set beside, shaped in the note's own
    /// style. It hangs in the indent to the left of the note, the way
    /// the marker of a list item does.
    fn note_marker(
        &self,
        style: &ComputedStyle,
        number: u32,
        x: f32,
        measure: f32,
    ) -> Option<Marker> {
        let text = style.list_style_type.marker(number)?;
        let line = self.line_of(&text, style.paragraph())?;
        let (left, _) = style.content_box(x, measure);
        let x = left - self.line_width(&line);
        Some(Marker { line, x })
    }

    /// What the footnote area is styled by.
    pub(super) fn area_style(&self) -> &ComputedStyle {
        self.styles.style(self.styles.notes_area())
    }

    /// Takes what the notes of the book are numbered. `paginate`
    /// reads it from the book it is handed. A caller that builds one
    /// section's fragments on its own sets it here.
    pub(crate) fn number(&self, numbering: Numbering) {
        *self.notes.borrow_mut() = numbering;
    }
}

/// The area one page sets its notes in: where the notes go, and what
/// the box around them paints.
pub(super) struct Area {
    /// Top of the area's border box, from the content box's top.
    pub(super) top: f32,
    /// What the whole area takes, the box's own edges included.
    pub(super) height: f32,
    /// The fragments set in it: the note each one is of, which of its
    /// fragments it is, and its top from the top of the area's
    /// content box.
    pub(super) placed: Vec<(f32, Arc<Note>, usize)>,
    /// The notes the page could not set, each with the fragment of it
    /// the next page opens with.
    pub(super) left: Vec<(Arc<Note>, usize)>,
}

/// What the area's own box takes above and below the notes in it.
pub(super) fn area_edges(style: &ComputedStyle) -> (f32, f32) {
    let border = style.border.widths();
    (
        style.margin.top + border.top + style.padding.top,
        style.margin.bottom + border.bottom + style.padding.bottom,
    )
}

impl Paginator<'_> {
    /// What the notes of one page come to: the area they fill from
    /// the foot of the content box upwards, and the note the page
    /// could not finish.
    ///
    /// `foot` is where the content of the page ends and `height` is
    /// the content box. Notes are set in the order their references
    /// were, from the first fragment each one still owes, and the
    /// area takes as many as the room left holds.
    pub(super) fn area(
        &self,
        notes: &[(Arc<Note>, usize)],
        foot: f32,
        height: f32,
    ) -> Option<Area> {
        if notes.is_empty() {
            return None;
        }
        let style = self.area_style();
        let (above, below) = area_edges(style);
        let room = height - foot - above - below;
        let mut placed: Vec<(f32, Arc<Note>, usize)> = Vec::new();
        let mut cursor = 0.0f32;
        let mut owed: Vec<(Arc<Note>, usize)> = Vec::new();
        for (note, from) in notes {
            if !owed.is_empty() {
                owed.push((note.clone(), *from));
                continue;
            }
            let mut at = *from;
            while at < note.fragments.len() {
                let step = note.step(at, !placed.is_empty());
                // The area takes one fragment whatever room is left,
                // the way a fragment taller than a page is set on it
                // and overflows it.
                if cursor + step > room && !placed.is_empty() {
                    break;
                }
                cursor += step;
                placed.push((cursor - note.fragments[at].height, note.clone(), at));
                at += 1;
            }
            if at < note.fragments.len() {
                owed.push((note.clone(), at));
            }
        }
        if placed.is_empty() {
            return Some(Area {
                top: height,
                height: 0.0,
                placed,
                left: owed,
            });
        }
        let outer = above + cursor + below;
        Some(Area {
            top: (height - outer).max(foot),
            height: outer,
            placed,
            left: owed,
        })
    }

    /// What one page's area paints, in page coordinates, and the
    /// border box every block of it takes there. `origin` is the
    /// page's content box.
    pub(super) fn area_items(
        &self,
        area: &Area,
        origin: (f32, f32),
    ) -> (Vec<DrawItem>, Vec<(NodeId, PageBox)>) {
        let style = self.area_style();
        let (above, _) = area_edges(style);
        let (x, _) = self.note_measure();
        let (left, width) = style.border_box(0.0, self.note_width());
        let top = area.top + style.margin.top;
        let height = (area.height - style.margin.top - style.margin.bottom).max(0.0);
        let ink = |edge: crate::style::Border| edge.color.unwrap_or(style.color);
        let mut items = super::flow::box_items(
            origin.0 + left,
            origin.1 + top,
            width,
            height,
            style.border_radius.resolve(width, height),
            style.border.widths(),
            crate::style::Edges {
                top: ink(style.border.top),
                right: ink(style.border.right),
                bottom: ink(style.border.bottom),
                left: ink(style.border.left),
            },
            &self.backdrop(&style.background),
            style.z_index,
        );
        let inner = (origin.0 + x, origin.1 + area.top + above);
        let placed: Vec<(f32, &Fragment)> = area
            .placed
            .iter()
            .map(|(at, note, index)| (*at, &note.fragments[*index]))
            .collect();
        let (mut boxes, mut areas) = decorate(&placed);
        super::flow::shift(&mut boxes, inner.0, inner.1);
        super::flow::shift_boxes(&mut areas, inner.0, inner.1);
        items.append(&mut boxes);
        for (at, fragment) in placed {
            items.append(&mut self.fragment_items(fragment, inner.0, inner.1 + at));
        }
        (items, areas)
    }
}

impl Flow<'_, '_> {
    /// Where the column being filled has to stop: the content box,
    /// less what the notes of that column need at the foot of it.
    /// `fragment` is the one about to be placed, whose own notes join
    /// them.
    ///
    /// This is the push-back: a note makes the column shorter, and a
    /// line that no longer fits under it moves on and takes the note
    /// its reference holds with it.
    pub(super) fn room(&self, fragment: &Fragment) -> f32 {
        let coming = fragment.notes.as_deref().map(Vec::as_slice).unwrap_or(&[]);
        let owed = self.owed(self.column);
        if owed.is_empty() && coming.is_empty() {
            return self.height;
        }
        let (above, below) = area_edges(self.paginator.area_style());
        let mut wanted = above + below;
        for (index, (note, from)) in owed.iter().enumerate() {
            wanted += note.height(*from, index > 0);
        }
        let under = !owed.is_empty();
        for (index, note) in coming.iter().enumerate() {
            wanted += note.height(0, under || index > 0);
        }
        (self.height - wanted).max(0.0)
    }

    /// The notes one column of the page being built owes, in the
    /// order their references were set.
    fn owed(&self, column: u32) -> Vec<(Arc<Note>, usize)> {
        self.notes
            .iter()
            .filter(|(at, _, _)| *at == column)
            .map(|(_, note, from)| (note.clone(), *from))
            .collect()
    }

    /// What the notes of the page being closed come to: one area
    /// under each column that owes any, with `placed` the fragments
    /// on the page.
    ///
    /// A column sets its notes under the foot of its own text. What
    /// one column cannot hold is set under the next, and what the
    /// last of them cannot hold opens the next page.
    pub(super) fn notes_areas(&self, placed: &[Placed]) -> Vec<(u32, Area)> {
        let mut areas = Vec::new();
        let mut left: Vec<(Arc<Note>, usize)> = Vec::new();
        for column in 0..self.columns.max(1) {
            let mut owed = std::mem::take(&mut left);
            owed.append(&mut self.owed(column));
            if owed.is_empty() {
                continue;
            }
            let foot = placed
                .iter()
                .filter(|placed| placed.column == column)
                .map(|placed| placed.top + placed.height)
                .fold(0.0, f32::max);
            let Some(area) = self.paginator.area(&owed, foot, self.height) else {
                continue;
            };
            left = area.left.clone();
            areas.push((column, area));
        }
        areas
    }

    /// Records the page the references of its notes were set on, and
    /// hands what the page could not set to the next one.
    pub(super) fn notes_closed(&mut self, areas: &[(u32, Area)]) {
        let page = self.pages.len() as u32;
        for (_, note, from) in &self.notes {
            if *from == 0 {
                self.note_pages.insert(note.node, page);
            }
        }
        self.notes = areas
            .last()
            .map(|(_, area)| area.left.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|(note, from)| (0, note, from))
            .collect();
    }
}
