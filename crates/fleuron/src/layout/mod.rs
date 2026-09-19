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
//!
//! The stages have a file each: `fragment` is what the flow moves,
//! `build` turns blocks into fragments, `cap` sets the initial letter
//! beside them, `list` sets a list an item at a time with the marker
//! of each, `table` sets a table a row at a time, `flow` stacks
//! fragments into pages, `exclusion` places the images and blocks the
//! sheet anchored and wraps prose around them, `background` puts art
//! behind a box, `inline` paints the box an inline element takes on a
//! line, `furniture` paints the margin boxes, and `text` turns a
//! shaped line into paint ops.

mod background;
mod build;
mod cap;
mod exclusion;
mod flow;
mod fragment;
mod furniture;
mod image;
mod inline;
mod list;
mod navigation;
mod note;
mod reference;
mod table;
mod text;

#[cfg(test)]
mod testing;

pub use build::Reflow;
pub use fragment::{
    BreakPoint, Decoration, Decorations, DropCap, Fragment, Marker, Marks, Piece, TableRow,
};
pub use furniture::margin_band;
pub use note::Note;

use background::Backdrop;

pub(crate) use exclusion::AnchoredBoxes;
pub(crate) use flow::{PageInfo, Paged};
pub(crate) use navigation::{navigation, run_area};
pub(crate) use note::Numbering;
pub(crate) use reference::{Named, References, landed, moved};

use std::borrow::Cow;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeSet;

use crate::content::{Book, Metadata};
use crate::fonts::FontRegistry;
use crate::images::{Assets, Contours};
use crate::lines::{LineLayout, Patterns};
use crate::pages::{Page, Side};
use crate::session::Session;
use crate::style::{Background, PageStyle, Position, StyleTree};
use crate::{LayoutOutput, Warning};

use flow::{Flow, PageSlot};

/// How many times a book whose notes are numbered by page is laid
/// out again for them. Each pass numbers the notes of the pass
/// before, and a book that has not settled by the last of them keeps
/// the numbers it has.
const NOTE_PASSES: u32 = 4;

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

/// The contours of a book whose sheet asked for none.
pub(crate) fn no_contours() -> &'static Contours {
    static EMPTY: std::sync::OnceLock<Contours> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Contours::none)
}

/// The fonts a run used, in the order the output indexes them.
pub(crate) fn font_table(registry: &FontRegistry) -> Vec<crate::fonts::FontRefEntry> {
    (0..registry.len() as u16)
        .filter_map(|id| registry.font_ref(id).cloned())
        .collect()
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
    /// What the trace stage made of the assets the sheet asked to
    /// wrap around.
    contours: &'a Contours,
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
    /// Whether the sheet anchors anything to the page, answered once.
    wraps: OnceCell<bool>,
    /// How many times the flow set a paragraph again beside an image.
    rebreaks: Cell<u32>,
    /// What the references in the book resolve against.
    references: RefCell<References>,
    /// What the notes of the book are numbered.
    notes: RefCell<Numbering>,
    /// How many times the book was laid out again to print the pages
    /// its references name.
    settles: Cell<u32>,
}

impl<'a> Paginator<'a> {
    /// A paginator over one book's styling and the faces it shapes
    /// with, for a book with no images in it.
    pub fn new(registry: &'a FontRegistry, styles: &'a StyleTree) -> Self {
        Paginator::with_assets(registry, styles, no_assets())
    }

    /// The same, over images the host has already probed. A book
    /// whose sheet names a traced contour also passes what the trace
    /// stage made of it, through
    /// [`with_contours`](Paginator::with_contours).
    pub fn with_assets(
        registry: &'a FontRegistry,
        styles: &'a StyleTree,
        assets: &'a Assets,
    ) -> Self {
        Paginator::with_contours(registry, styles, assets, no_contours())
    }

    /// The same, over the contours the trace stage left.
    pub fn with_contours(
        registry: &'a FontRegistry,
        styles: &'a StyleTree,
        assets: &'a Assets,
        contours: &'a Contours,
    ) -> Self {
        Paginator {
            registry,
            styles,
            assets,
            contours,
            lines: LineLayout::new(registry),
            patterns: Cell::new(Patterns::default()),
            unknown: RefCell::new(None),
            warnings: RefCell::new(Vec::new()),
            wraps: OnceCell::new(),
            rebreaks: Cell::new(0),
            references: RefCell::new(References::default()),
            notes: RefCell::new(Numbering::default()),
            settles: Cell::new(0),
        }
    }
}

impl Paginator<'_> {
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
                format!(
                    "No hyphenation patterns for `{tag}`. Hyphenation is skipped for that \
                     language."
                ),
                None,
            );
        }
        self.patterns.get()
    }

    /// What fragmentation had to complain about.
    pub fn warnings(&self) -> Vec<Warning> {
        self.warnings.borrow().clone()
    }

    /// How many times the flow that paints set a paragraph again
    /// beside an image. A book that anchors nothing never does.
    pub fn rebreaks(&self) -> u32 {
        self.rebreaks.get()
    }

    /// How many times the book was laid out a second time, to print
    /// the pages its references name. A book whose sheet prints no
    /// page number is laid out once.
    pub fn settles(&self) -> u32 {
        self.settles.get()
    }

    /// Takes what the book's references resolve against. `paginate`
    /// reads it from the book it is handed. A caller that builds one
    /// section's fragments on its own sets it here.
    pub(crate) fn refer(&self, references: References) {
        *self.references.borrow_mut() = references;
    }

    /// Whether the sheet takes anything out of the flow and against
    /// the page.
    ///
    /// A book that anchors nothing never sets a paragraph twice, so
    /// its fragments keep nothing to set one from.
    fn wraps(&self) -> bool {
        *self.wraps.get_or_init(|| {
            self.styles
                .styles()
                .iter()
                .any(|style| style.position == Position::Absolute)
        })
    }

    /// Records one diagnostic, once. A book that hits the same
    /// problem on every page has one problem.
    fn warn(&self, message: String, origin: Option<String>) {
        let mut warnings = self.warnings.borrow_mut();
        if !warnings.iter().any(|seen| seen.message == message) {
            warnings.push(Warning { message, origin });
        }
    }

    /// Says so where the host supplied no image for a url. The table
    /// complains about a url it probed and refused. A url the table
    /// was never offered means the host supplied nothing at all.
    /// What one box paints behind its content, complaining where the
    /// sheet named an image nothing answers for.
    fn backdrop(&self, background: &Background) -> Backdrop {
        let found = background.image.as_ref().and_then(|url| {
            let found = self.assets.lookup(&url.value);
            if found.is_none() {
                self.missing(&url.value, url.origin.clone().unwrap_or_default());
            }
            found
        });
        Backdrop::of(background, found)
    }

    fn missing(&self, url: &str, origin: String) {
        if !self.assets.probed(url) {
            self.warn(
                format!("No image was supplied for {url}. The image is skipped."),
                (!origin.is_empty()).then_some(origin),
            );
        }
    }

    /// Flows one book into numbered, side-tagged pages.
    ///
    /// A section's fragments are built, flowed, and released before
    /// the next one is measured: what exists at once is the book's
    /// pages, not every line it was ever broken into.
    ///
    /// A book whose references print pages is laid out twice: once to
    /// find the page each element lands on, and once to print it.
    pub fn paginate(&self, book: &Book) -> Vec<Page> {
        self.paginated(book).pages
    }

    /// The same, with where each id landed and the box of each block.
    pub(crate) fn paginated(&self, book: &Book) -> Paged {
        self.language(&book.metadata);
        if self.styles.refers() {
            self.refer(References::of(book));
        }
        self.number(Numbering::of(book, self.styles));
        let mut paged = self.pass(book);
        if self.styles.numbers_notes_per_page() {
            paged = self.settle_notes(book, paged);
        }
        if self.styles.counts_pages() {
            let found = landed(&paged);
            let resolved = self.references.borrow().landed(found.clone());
            self.refer(resolved);
            self.settles.set(self.settles.get() + 1);
            paged = self.pass(book);
            let references = self.references.borrow();
            let printed: BTreeSet<_> = book
                .sections
                .iter()
                .flat_map(|section| Named::in_section(section, self.styles, &references).pages)
                .collect();
            for warning in moved(&found, &landed(&paged), &printed, &references) {
                self.warn(warning.message, warning.origin);
            }
        }
        self.paint(&mut paged.pages, &paged.infos);
        paged
    }

    /// Numbers the notes by the page their references were set on,
    /// and lays the book out again to print those numbers.
    ///
    /// A number of another width moves the line its reference is on,
    /// which can move a note onto another page and number it again.
    /// So the book is laid out until the numbering stops changing,
    /// and the numbering of the last pass stands where it does not.
    fn settle_notes(&self, book: &Book, mut paged: Paged) -> Paged {
        let start = self.styles.first_note_number();
        for _ in 0..NOTE_PASSES {
            let numbered = self.notes.borrow().on_pages(&paged.notes, start);
            if numbered == *self.notes.borrow() {
                return paged;
            }
            self.number(numbered);
            self.settles.set(self.settles.get() + 1);
            paged = self.pass(book);
        }
        self.warn(
            concat!(
                "The notes were numbered by page and the numbering did not settle. ",
                "The numbers are the ones of the last pass.",
            )
            .to_string(),
            None,
        );
        paged
    }

    /// One pass over the whole book, stopping short of the furniture.
    fn pass(&self, book: &Book) -> Paged {
        let anchored = self.anchored(book, |index| {
            Cow::Owned(self.section_fragments(&book.sections[index]))
        });
        let mut flow = Flow::new(self, anchored);
        for section in &book.sections {
            let fragments = self.section_fragments(section);
            flow.section(section, &fragments);
        }
        flow.finish()
    }

    /// The boxes the sheet anchors, each on the page its anchor
    /// landed on and against the block its insets measure from.
    ///
    /// A box inside a positioned block is laid out a second time: the
    /// first layout breaks its lines to what the page area leaves it,
    /// and the settling pass answers which block they measure from and
    /// how wide that block is.
    fn anchored<'f>(
        &self,
        book: &Book,
        mut fragments: impl FnMut(usize) -> Cow<'f, [Fragment]>,
    ) -> AnchoredBoxes {
        let boxes = self.anchored_boxes(book, &[]);
        if boxes.is_empty() {
            return AnchoredBoxes::default();
        }
        let mut settled = self.settle(book, boxes, &mut fragments);
        if let Some(within) = settled.narrowed() {
            let boxes = self.anchored_boxes(book, &within);
            settled.relaid(boxes, within);
        }
        settled
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
        let sections: Vec<&[Fragment]> = sections.into_iter().collect();
        let anchored = self.anchored(book, |index| Cow::Borrowed(sections[index]));
        let mut flow = Flow::new(self, anchored);
        for (section, fragments) in book.sections.iter().zip(&sections) {
            flow.section(section, fragments);
        }
        flow.finish()
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
            links: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::testing::{book_of, heading, long_prose, registry, section};

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
