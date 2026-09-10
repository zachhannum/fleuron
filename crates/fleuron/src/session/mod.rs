//! A retained pipeline: every stage kept, and only what an edit
//! changed run again.
//!
//! `layout_book` is a pure function of its inputs, so every call
//! rebuilds every stage. A process that renders one book and exits
//! wants that. A live preview does not, because the common event
//! there is a small change to one input while the others stand. A
//! session retains the output of each stage and works out the deepest
//! stage an edit reaches: a colour serves the display structure back, page
//! furniture repaints, `@page` geometry re-fragments over cached
//! lines, and only the measure or the text itself breaks lines
//! again.
//!
//! # Section-local lines
//!
//! Line *breaking* is section-local: where the breaks fall depends on
//! the measure, the face and the text, not on where the section
//! starts vertically. Line *placement* depends on position and
//! belongs to fragmentation. So the cache stores breaks, shaped runs
//! and advances, and no page coordinates at all. An edit to one
//! chapter re-breaks that chapter and re-fragments the book, which
//! for a whole novel costs about what tracking the pages that moved
//! would.
//!
//! Two preconditions make that sound, and both are checked rather
//! than remembered. The first is a uniform measure: masters that
//! resolve different content widths make breaking depend on which
//! page a line lands on. The second is that no inline text depends
//! on pagination, which the parser guarantees: `counter(page)` is
//! legal only inside a margin box. When either one fails, the session re-breaks
//! everything instead of serving stale lines.
//!
//! The parts have a file each: `edit` is what a host changes,
//! `output` is what it asks for, `stage` runs the stages, `invalidate`
//! decides which of them an edit reaches, and `key` fingerprints one
//! section.

use std::borrow::Cow;

use crate::content::{Book, NodeId};
use crate::fonts::{FontError, FontRegistry};
use crate::images::{Assets, Contours};
use crate::layout::{Fragment, PageInfo, Piece, no_assets};
use crate::style::{StyleTree, Stylesheets};
use crate::{LayoutOutput, Warning};

mod edit;
mod invalidate;
mod key;
mod output;
mod stage;

#[cfg(test)]
mod testing;

use invalidate::{Prints, section_local};

/// How many times each stage has run since the session was made.
///
/// A host reads these to see what an edit cost. The tests read them
/// to prove what an edit did *not* cost, which a clock cannot show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stages {
    /// Style compilations: parse, match, cascade.
    pub style: u32,
    /// Images decoded to trace a contour. A book whose sheet names no
    /// contour never decodes one, and a contour already traced is not
    /// traced again.
    pub trace: u32,
    /// Sections broken into lines. One per section, per rebuild.
    pub lines: u32,
    /// Fragmentation and page assembly runs.
    pub flow: u32,
    /// Furniture paints: numbering and margin boxes.
    pub paint: u32,
}

/// The deepest stage a change invalidates, which is the shallowest
/// cache that survives it. Ordered: a deeper stage implies every
/// stage under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stale {
    /// Everything stands; the display structure is served as it is.
    Nothing,
    /// Numbering and margin boxes.
    Paint,
    /// Fragmentation, over lines that survived.
    Flow,
    /// Line breaking, and everything below it.
    Break,
    /// Tracing the contours the sheet asks for, which is above line
    /// breaking because a contour that moved is a measure that moved.
    Trace,
}

/// A table a session lays out against: a caller's, or one of its
/// own.
///
/// A host with faces and images to lend keeps lending them. A worker
/// has nowhere to keep either, since the module is all there is, so
/// it hands them over once and adds to them through the session.
enum Table<'a, T> {
    Borrowed(&'a T),
    Owned(Box<T>),
}

impl<T> Table<'_, T> {
    fn get(&self) -> &T {
        match self {
            Table::Borrowed(table) => table,
            Table::Owned(table) => table,
        }
    }

    fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Table::Borrowed(_) => None,
            Table::Owned(table) => Some(table),
        }
    }
}

/// Why a face did not reach a session's registry.
#[derive(Debug, thiserror::Error)]
pub enum AddFontError {
    /// The session lays out against a registry it borrowed, and the
    /// caller who owns it is the one who can add to it.
    #[error("the session borrows its font registry")]
    Borrowed,
    /// The bytes are not a face this build can read.
    #[error(transparent)]
    Font(#[from] FontError),
}

/// Why an image did not reach a session's asset table.
#[derive(Debug, thiserror::Error)]
pub enum AddImageError {
    /// The session lays out against an asset table it borrowed, and
    /// the caller who owns it is the one who can add to it.
    #[error("the session borrows its asset table")]
    Borrowed,
}

/// One section's lines, and what building them had to complain about.
struct Cached {
    key: u64,
    /// The id the section had when its lines were broken.
    section: NodeId,
    fragments: Vec<Fragment>,
    warnings: Vec<Warning>,
}

impl Cached {
    /// Moves the source ranges onto the ids the book hands out now.
    /// Ids renumber globally on every edit, so a chapter nothing
    /// touched comes back out of the cache under a new number; its
    /// nodes are dense and in document order from the section's own,
    /// so all of them move by the same step.
    fn renumber(&mut self, section: NodeId) {
        let step = section.get() as i64 - self.section.get() as i64;
        self.section = section;
        if step == 0 {
            return;
        }
        for fragment in &mut self.fragments {
            if let Piece::Anchor(node) = &mut fragment.piece {
                *node = node.shifted(step);
            }
            let Piece::Line { line, cap } = &mut fragment.piece else {
                continue;
            };
            let caps = cap.iter_mut().map(|cap| &mut cap.line);
            for line in std::iter::once(line).chain(caps) {
                for origin in line.runs.iter_mut().filter_map(|run| run.origin.as_mut()) {
                    origin.node = origin.node.shifted(step);
                }
            }
        }
    }
}

/// A retained pipeline: content, styling, and every stage between
/// them and the page.
///
/// ```
/// # use fleuron::content::Book;
/// # use fleuron::session::Session;
/// # use fleuron::style::Stylesheets;
/// # let registry = fleuron::fonts::bundled_registry().unwrap();
/// let mut session = Session::new(&registry);
/// session.set_content(Book::default());
/// session.set_style(Stylesheets::parse(&[]));
/// let pages = &session.preview().pages;
/// ```
///
/// The registry is fixed for the session's life. A computed style
/// can only resolve to a face already in the registry, so a sheet
/// that brings its own `@font-face` needs the host to register that
/// face before the sheet is set.
pub struct Session<'a> {
    registry: Table<'a, FontRegistry>,
    assets: Table<'a, Assets>,
    /// What the trace stage made of the assets the sheet wraps prose
    /// around. Empty for a sheet that names no contour.
    contours: Contours,
    book: Cow<'a, Book>,
    /// The sheets the tree was compiled from. `None` on the one-shot
    /// path, where the caller compiled the tree itself and nothing
    /// will ask for another.
    sheets: Option<Stylesheets>,
    styles: Cow<'a, StyleTree>,
    prints: Prints,
    /// Whether the preconditions for reusing a section's lines are met.
    section_local: bool,
    /// Whether the book places an image, which is what makes the
    /// page's height an input to breaking.
    images: bool,
    /// Whether the stages are kept between calls. This is the only
    /// difference on the one-shot path, which keeps one section's
    /// lines at a time and drops each as it is flowed.
    retain: bool,
    lines: Vec<Cached>,
    infos: Vec<PageInfo>,
    output: Option<LayoutOutput>,
    /// What building lines complained about, deduped in the order the
    /// sections raised it.
    flow_warnings: Vec<Warning>,
    /// What a frontend had to say about the sources it read, which
    /// happened upstream of every stage here.
    source_warnings: Vec<Warning>,
    stale: Stale,
    stages: Stages,
}

impl<'a> Session<'a> {
    /// A session over the faces in `registry`, with no content and
    /// the built-in sheet alone.
    pub fn new(registry: &'a FontRegistry) -> Session<'a> {
        Session::with_assets(registry, no_assets())
    }

    /// The same, over images the host has already probed.
    pub fn with_assets(registry: &'a FontRegistry, assets: &'a Assets) -> Session<'a> {
        Session::over(Table::Borrowed(registry), Table::Borrowed(assets))
    }

    fn over(registry: Table<'a, FontRegistry>, assets: Table<'a, Assets>) -> Session<'a> {
        let book = Book::default();
        let sheets = Stylesheets::parse(&[]);
        let styles = sheets.compile(&book, registry.get());
        Session {
            registry,
            assets,
            contours: Contours::none(),
            book: Cow::Owned(book),
            sheets: Some(sheets),
            prints: Prints::of(&styles, false),
            section_local: section_local(&styles),
            images: false,
            styles: Cow::Owned(styles),
            retain: true,
            lines: Vec::new(),
            infos: Vec::new(),
            output: None,
            flow_warnings: Vec::new(),
            source_warnings: Vec::new(),
            stale: Stale::Trace,
            stages: Stages {
                style: 1,
                ..Stages::default()
            },
        }
    }

    /// A session that owns the faces it lays out against, and takes
    /// more through [`add_font`](Session::add_font).
    ///
    /// This is the shape a worker needs: font bytes cross the
    /// boundary once, the module keeps them, and no caller on the
    /// other side of the wall has a registry to lend.
    pub fn owning(registry: FontRegistry) -> Session<'static> {
        Session::over(
            Table::Owned(Box::new(registry)),
            Table::Owned(Box::new(Assets::none())),
        )
    }

    /// The single run `layout_book` makes, over inputs the caller
    /// owns and will not edit. Nothing is fingerprinted, because
    /// nothing will be compared against it.
    pub(crate) fn once(
        book: &'a Book,
        styles: &'a StyleTree,
        registry: &'a FontRegistry,
        assets: &'a Assets,
    ) -> Session<'a> {
        Session {
            registry: Table::Borrowed(registry),
            assets: Table::Borrowed(assets),
            contours: Contours::none(),
            book: Cow::Borrowed(book),
            sheets: None,
            styles: Cow::Borrowed(styles),
            prints: Prints::default(),
            section_local: false,
            images: false,
            retain: false,
            lines: Vec::new(),
            infos: Vec::new(),
            output: None,
            flow_warnings: Vec::new(),
            source_warnings: Vec::new(),
            stale: Stale::Trace,
            stages: Stages::default(),
        }
    }
}

impl Session<'_> {
    /// The session's own copy of the book, node ids assigned.
    pub fn book(&self) -> &Book {
        &self.book
    }

    /// The compiled styling behind the last update.
    pub fn styles(&self) -> &StyleTree {
        &self.styles
    }

    /// The faces this session lays out against.
    ///
    /// A painter that has to draw with the same file the shaper used
    /// reaches the bytes through here; the display structure names
    /// ids, and the registry is what they index.
    pub fn fonts(&self) -> &FontRegistry {
        self.registry.get()
    }

    /// How many times each stage has run.
    pub fn stages(&self) -> Stages {
        self.stages
    }

    /// Whether a section's lines survive an edit elsewhere in the
    /// book. This goes false when the styling breaks a precondition,
    /// either masters of different measures or inline content that
    /// depends on pagination, and everything is re-broken instead.
    pub fn reuses_sections(&self) -> bool {
        self.section_local
    }
}
