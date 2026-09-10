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
//! beside them, `flow` stacks fragments into pages, `exclusion` sets
//! prose around an image the sheet anchored, `furniture` paints the
//! margin boxes, and `text` turns a shaped line into paint ops.

mod build;
mod cap;
mod exclusion;
mod flow;
mod fragment;
mod furniture;
mod text;

#[cfg(test)]
mod testing;

pub use build::Reflow;
pub use fragment::{BreakPoint, Decoration, Decorations, DropCap, Fragment, Marks, Piece};
pub use furniture::margin_band;

pub(crate) use exclusion::AnchoredImages;
pub(crate) use flow::{PageInfo, Paged};

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeMap;

use crate::content::{Book, Metadata};
use crate::fonts::FontRegistry;
use crate::images::{Assets, Contours};
use crate::lines::{LineLayout, Patterns};
use crate::pages::{Page, Side};
use crate::session::Session;
use crate::style::{PageStyle, Position, StyleTree};
use crate::{LayoutOutput, Warning};

use flow::{Flow, PageSlot};

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

    /// How many times the flow set a paragraph again beside an image.
    /// A book that anchors nothing never does.
    pub fn rebreaks(&self) -> u32 {
        self.rebreaks.get()
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
    fn missing(&self, url: &str, origin: String) {
        if !self.assets.probed(url) {
            self.warn(
                format!("image {url}: no image was supplied for it; it is skipped"),
                (!origin.is_empty()).then_some(origin),
            );
        }
    }

    /// Flows one book into numbered, side-tagged pages.
    ///
    /// A section's fragments are built, flowed, and released before
    /// the next one is measured: what exists at once is the book's
    /// pages, not every line it was ever broken into.
    pub fn paginate(&self, book: &Book) -> Vec<Page> {
        self.language(&book.metadata);
        let anchored = self.anchored_images(book);
        // The pass that answers where the anchors land keeps no
        // fragments either. It builds a section, flows it, and drops
        // it, the same way the pass that keeps the pages does.
        let bare = AnchoredImages::default();
        let anchors = if anchored.is_empty() {
            BTreeMap::new()
        } else {
            let mut flow = Flow::settling(self, &bare);
            for section in &book.sections {
                let fragments = self.section_fragments(section);
                flow.section(section, &fragments);
            }
            flow.finish().anchors
        };
        let anchored = AnchoredImages::on(anchored, &anchors);
        let mut flow = Flow::new(self, &anchored);
        for section in &book.sections {
            let fragments = self.section_fragments(section);
            flow.section(section, &fragments);
        }
        let mut paged = flow.finish();
        self.paint(&mut paged.pages, &paged.infos);
        paged.pages
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
        let anchored = self.anchored_images(book);
        let bare = AnchoredImages::default();
        let anchored = if anchored.is_empty() {
            AnchoredImages::default()
        } else {
            let mut flow = Flow::settling(self, &bare);
            for (section, fragments) in book.sections.iter().zip(&sections) {
                flow.section(section, fragments);
            }
            AnchoredImages::on(anchored, &flow.finish().anchors)
        };
        let mut flow = Flow::new(self, &anchored);
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
