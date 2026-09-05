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

use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::content::{Block, Book, Inline, Metadata, NodeId, Section};
use crate::fonts::{FontError, FontRegistry, FontSource};
use crate::images::Assets;
use crate::layout::{Fragment, PageInfo, Paginator, font_table, no_assets};
use crate::pdf::{self, PdfError};
use crate::style::{ComputedStyle, Content, Edges, PageGeometry, StyleTree, Stylesheets};
use crate::{LayoutOutput, Warning};

/// How many times each stage has run since the session was made.
///
/// A host reads these to see what an edit cost. The tests read them
/// to prove what an edit did *not* cost, which a clock cannot show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stages {
    /// Style compilations: parse, match, cascade.
    pub style: u32,
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
    fragments: Vec<Fragment>,
    warnings: Vec<Warning>,
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
            stale: Stale::Break,
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
            stale: Stale::Break,
            stages: Stages::default(),
        }
    }

    /// Sets the book, and with it every stage below box
    /// construction.
    ///
    /// Node identity is the engine's: the tree is renumbered on the
    /// way in, so a host may hand over sections it built by hand.
    pub fn set_content(&mut self, mut book: Book) {
        book.assign_node_ids();
        self.book = Cow::Owned(book);
        self.images = has_images(&self.book);
        self.recompile();
        self.stale = Stale::Break;
    }

    /// Replaces every section that came from one source file.
    ///
    /// The source is the replaceable unit because a host names files
    /// and one file may split into several sections. A name the book
    /// does not already have appends instead, which is how a file it
    /// has not seen before arrives.
    pub fn replace_source(&mut self, name: &str, sections: Vec<Section>) {
        let mut sections = sections;
        let book = self.book.to_mut();
        let mut rebuilt = Vec::with_capacity(book.sections.len() + sections.len());
        let mut placed = false;
        for section in std::mem::take(&mut book.sections) {
            if section.source.as_deref() == Some(name) {
                if !placed {
                    rebuilt.append(&mut sections);
                    placed = true;
                }
            } else {
                rebuilt.push(section);
            }
        }
        if !placed {
            rebuilt.append(&mut sections);
        }
        book.sections = rebuilt;
        book.assign_node_ids();
        self.images = has_images(&self.book);
        self.recompile();
        self.stale = Stale::Break;
    }

    /// Adds a frontend's complaints to the run's diagnostics.
    ///
    /// A construct the content vocabulary cannot express is reported
    /// where the source was read, which is upstream of every stage a
    /// session runs. The output's warnings are the whole run's, so
    /// they belong in the same channel rather than in a second one
    /// the host has to remember to read. Which of them still apply
    /// after an edit is the caller's to decide; this replaces the
    /// lot.
    pub fn set_source_warnings(&mut self, warnings: Vec<Warning>) {
        self.source_warnings = warnings;
    }

    /// Sets the styling, and with it whichever stage the change
    /// reaches, which is usually far short of everything.
    pub fn set_style(&mut self, sheets: Stylesheets) {
        self.sheets = Some(sheets);
        self.recompile();
    }

    /// Registers a face, and re-runs everything a face can change.
    ///
    /// A family the registry did not have is a family the cascade
    /// resolved to something else, so the styling is compiled again
    /// and the lines are broken again. The output's font table is
    /// rebuilt with them.
    ///
    /// Only a session that owns its registry has one to add to; one
    /// that borrowed it says so instead.
    pub fn add_font(&mut self, source: FontSource) -> Result<Vec<u16>, AddFontError> {
        let ids = self
            .registry
            .get_mut()
            .ok_or(AddFontError::Borrowed)?
            .add(source)?;
        // The table is built with the output and never patched, so
        // the output goes rather than outlive the ids it indexes.
        self.output = None;
        self.stale = Stale::Break;
        self.recompile();
        Ok(ids)
    }

    /// Registers one image, and the index `DrawItem::Image.asset`
    /// gets for it. `None` for bytes no probe recognises,
    /// which is a diagnostic on the next display structure and no asset.
    ///
    /// The header decides how much room the image takes, so the
    /// lines are broken again. Only a session that owns its asset
    /// table has one to add to; one that borrowed it says so
    /// instead.
    pub fn add_image(&mut self, url: &str, bytes: Vec<u8>) -> Result<Option<u32>, AddImageError> {
        if let Some((index, _)) = self.assets.get().lookup(url) {
            // A url already registered is the image the pages were
            // laid out around, and re-registering it costs nothing.
            return Ok(Some(index));
        }
        let index = self
            .assets
            .get_mut()
            .ok_or(AddImageError::Borrowed)?
            .add(url, bytes);
        // The table is built with the output and never patched, so
        // the output goes rather than outlive the indexes it names.
        self.output = None;
        self.stale = Stale::Break;
        Ok(index)
    }

    /// The display structure, brought up to date.
    pub fn preview(&mut self) -> &LayoutOutput {
        self.update();
        self.output.as_ref().expect("an update leaves an output")
    }

    /// The same, as PDF bytes. The stages above the painter are the
    /// ones the preview used, so an export cannot contradict it.
    pub fn export(&mut self) -> Result<Vec<u8>, PdfError> {
        self.update();
        let output = self.output.as_ref().expect("an update leaves an output");
        pdf::write(
            output,
            self.registry.get(),
            self.assets.get(),
            &self.book.metadata,
        )
    }

    /// The display structure by value, consuming the session.
    pub fn into_output(mut self) -> LayoutOutput {
        self.update();
        self.output.take().expect("an update leaves an output")
    }

    /// Names the book: title, author, and whatever else a frontend
    /// read.
    ///
    /// Nothing between the content tree and the page reads metadata,
    /// so this invalidates no stage. The pages already laid out are
    /// the pages the export writes under the new name.
    pub fn set_metadata(&mut self, metadata: Metadata) {
        self.book.to_mut().metadata = metadata;
    }

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

    /// Compiles the styling again and classifies what moved.
    fn recompile(&mut self) {
        let Some(sheets) = &self.sheets else {
            return;
        };
        let styles = sheets.compile(&self.book, self.registry.get());
        self.stages.style += 1;
        let prints = Prints::of(&styles, self.images);
        self.stale = self.stale.max(self.prints.against(&prints));
        self.prints = prints;
        self.section_local = section_local(&styles);
        self.styles = Cow::Owned(styles);
    }

    /// Runs the stages the last change invalidated, and no others.
    fn update(&mut self) {
        if self.retain {
            if self.stale >= Stale::Break {
                self.rebreak();
            }
            if self.stale >= Stale::Flow {
                self.reflow();
            }
            if self.stale >= Stale::Paint {
                self.repaint();
            }
        } else if self.stale != Stale::Nothing {
            self.run_once();
        }
        self.stale = Stale::Nothing;
        self.collect_warnings();
    }

    /// Breaks the sections whose lines the cache cannot answer for,
    /// and keeps the rest as they stand.
    fn rebreak(&mut self) {
        let against = Against::of(&self.styles, self.images);
        // An image that arrives is a box that was not reserved
        // before it, so the sections are broken again around it. The
        // table only grows, so a count that moved is an image.
        let supplied = self.images.then(|| self.assets.get().assets().len());
        let mut previous: Vec<Option<Cached>> = std::mem::take(&mut self.lines)
            .into_iter()
            .map(Some)
            .collect();
        let mut spare: HashMap<u64, Vec<usize>> = HashMap::new();
        if self.section_local {
            for (index, cached) in previous.iter().enumerate() {
                let key = cached.as_ref().expect("nothing is taken yet").key;
                spare.entry(key).or_default().push(index);
            }
        }
        let mut fresh = Vec::with_capacity(self.book.sections.len());
        for section in &self.book.sections {
            let key = section_key(section, &self.styles, against, supplied);
            let kept = spare
                .get_mut(&key)
                .and_then(|slots| slots.pop())
                .and_then(|slot| previous[slot].take());
            fresh.push(match kept {
                Some(cached) => cached,
                None => {
                    // One paginator per section, so the warnings it
                    // collects are the ones this section raised.
                    let paginator = Paginator::with_assets(
                        self.registry.get(),
                        &self.styles,
                        self.assets.get(),
                    );
                    let fragments = paginator.section_fragments(section);
                    self.stages.lines += 1;
                    Cached {
                        key,
                        fragments,
                        warnings: paginator.warnings(),
                    }
                }
            });
        }
        self.lines = fresh;
        self.flow_warnings.clear();
        for cached in &self.lines {
            for warning in &cached.warnings {
                if !self
                    .flow_warnings
                    .iter()
                    .any(|seen| seen.message == warning.message)
                {
                    self.flow_warnings.push(warning.clone());
                }
            }
        }
    }

    /// Flows the cached lines into pages. Nothing here measures.
    fn reflow(&mut self) {
        let registry = self.registry.get();
        let assets = self.assets.get();
        let paginator =
            Paginator::with_assets(self.registry.get(), &self.styles, self.assets.get());
        let paged = paginator.fragment(
            &self.book,
            self.lines.iter().map(|cached| cached.fragments.as_slice()),
        );
        self.stages.flow += 1;
        self.infos = paged.infos;
        self.output
            .get_or_insert_with(|| blank_output(registry, assets))
            .pages = paged.pages;
    }

    /// Repaints the furniture over pages the flow already settled.
    fn repaint(&mut self) {
        let paginator =
            Paginator::with_assets(self.registry.get(), &self.styles, self.assets.get());
        if let Some(output) = &mut self.output {
            paginator.paint(&mut output.pages, &self.infos);
            self.stages.paint += 1;
        }
    }

    /// The whole pipeline, one section's lines alive at a time.
    fn run_once(&mut self) {
        let registry = self.registry.get();
        let assets = self.assets.get();
        let paginator =
            Paginator::with_assets(self.registry.get(), &self.styles, self.assets.get());
        let pages = paginator.paginate(&self.book);
        self.stages.lines += self.book.sections.len() as u32;
        self.stages.flow += 1;
        self.stages.paint += 1;
        self.flow_warnings = paginator.warnings();
        self.output
            .get_or_insert_with(|| blank_output(registry, assets))
            .pages = pages;
    }

    /// Everything the run has to complain about, in the order the
    /// stages raised it.
    fn collect_warnings(&mut self) {
        let mut warnings = self.source_warnings.clone();
        warnings.extend(self.styles.warnings().iter().cloned());
        warnings.extend(self.assets.get().warnings().iter().cloned());
        warnings.extend(self.flow_warnings.iter().cloned());
        if let Some(output) = &mut self.output {
            output.warnings = warnings;
        }
    }
}

/// An output with the font and asset tables filled in and nothing
/// painted yet.
fn blank_output(registry: &FontRegistry, assets: &Assets) -> LayoutOutput {
    LayoutOutput {
        pages: Vec::new(),
        fonts: font_table(registry),
        assets: assets.assets().to_vec(),
        warnings: Vec::new(),
    }
}

/// The page a section's lines were broken against: the measure
/// always, and the content height only for a book with an image in
/// it. An image is the one thing fragment building sizes against the
/// page's own height, so a book without one never reads that height,
/// and a page that grows taller leaves its prose broken where it
/// was.
#[derive(Debug, Clone, Copy)]
struct Against {
    measure: f32,
    height: Option<f32>,
}

impl Against {
    fn of(styles: &StyleTree, images: bool) -> Against {
        let geometry = styles.default_page().geometry;
        Against {
            measure: geometry.measure(),
            height: images.then(|| geometry.content_size().1),
        }
    }

    fn hash_into(self, h: &mut DefaultHasher) {
        (self.measure.to_bits(), self.height.map(f32::to_bits)).hash(h);
    }
}

/// Whether any section of the book places an image.
fn has_images(book: &Book) -> bool {
    fn walk(blocks: &[Block]) -> bool {
        blocks.iter().any(|block| match block {
            Block::Image { .. } => true,
            Block::Blockquote { blocks, .. } => walk(blocks),
            _ => false,
        })
    }
    book.sections.iter().any(|section| walk(&section.blocks))
}

/// What the compiled styling hashes to, split by the stage each part
/// feeds. A style edit is classified by which of these moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Prints {
    /// The measure, what every distinct style says about breaking,
    /// and which style each node resolved to.
    breaks: u64,
    /// Page geometry, and the named page each element asks for.
    flow: u64,
    /// The margin boxes, and what they paint.
    paint: u64,
}

impl Prints {
    fn of(styles: &StyleTree, images: bool) -> Prints {
        let mut breaks = DefaultHasher::new();
        Against::of(styles, images).hash_into(&mut breaks);
        for style in styles.styles() {
            hash_layout(style, &mut breaks);
        }
        hash_nodes(styles, &mut breaks);

        let mut flow = DefaultHasher::new();
        for master in styles.masters() {
            (&master.page, master.situation).hash(&mut flow);
            hash_geometry(master.style.geometry, &mut flow);
        }
        for style in styles.styles() {
            style.page.hash(&mut flow);
        }
        hash_nodes(styles, &mut flow);

        let mut paint = DefaultHasher::new();
        for master in styles.masters() {
            (&master.page, master.situation).hash(&mut paint);
            hash_geometry(master.style.geometry, &mut paint);
            for box_ in &master.style.boxes {
                (box_.which, &box_.content).hash(&mut paint);
                hash_layout(&box_.style, &mut paint);
            }
        }

        Prints {
            breaks: breaks.finish(),
            flow: flow.finish(),
            paint: paint.finish(),
        }
    }

    /// The deepest stage a move from `self` to `fresh` invalidates.
    fn against(&self, fresh: &Prints) -> Stale {
        if self.breaks != fresh.breaks {
            Stale::Break
        } else if self.flow != fresh.flow {
            Stale::Flow
        } else if self.paint != fresh.paint {
            Stale::Paint
        } else {
            Stale::Nothing
        }
    }
}

/// Whether a section's lines may outlive an edit elsewhere.
fn section_local(styles: &StyleTree) -> bool {
    uniform_measure(styles) && !paginated_prose(styles.styles())
}

/// Whether every master breaks to the same measure. One that does
/// not makes where a line breaks depend on which page it lands on,
/// and that on everything before it.
fn uniform_measure(styles: &StyleTree) -> bool {
    let measure = styles.default_page().geometry.measure().to_bits();
    styles
        .masters()
        .iter()
        .all(|master| master.style.geometry.measure().to_bits() == measure)
}

/// Whether any element generates text that only pagination resolves.
/// `counter(page)` and `string()` in prose would make inline text
/// depend on where the prose fell, which is a fixpoint rather than a
/// cache invalidation.
fn paginated_prose(styles: &[ComputedStyle]) -> bool {
    styles
        .iter()
        .any(|style| matches!(style.content, Content::Counter(_) | Content::String(_)))
}

/// What one section's lines were built from: the file it came from,
/// its content, and the style every node in it resolved to. Node ids
/// are deliberately absent, because they renumber globally on every
/// edit and a chapter nothing touched would then miss its own
/// cache.
fn section_key(
    section: &Section,
    styles: &StyleTree,
    against: Against,
    assets: Option<usize>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    let h = &mut hasher;
    against.hash_into(h);
    assets.hash(h);
    (&section.source, &section.title, section.position).hash(h);
    hash_node(section.id, styles, h);
    hash_blocks(&section.blocks, styles, h);
    hasher.finish()
}

fn hash_blocks(blocks: &[Block], styles: &StyleTree, h: &mut DefaultHasher) {
    for block in blocks {
        match block {
            Block::Heading {
                id,
                level,
                inlines,
                position,
            } => {
                (0u8, level, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(inlines, styles, h);
            }
            Block::Paragraph {
                id,
                inlines,
                position,
            } => {
                (1u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(inlines, styles, h);
            }
            Block::Blockquote {
                id,
                blocks,
                position,
            } => {
                (2u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_blocks(blocks, styles, h);
            }
            Block::ThematicBreak { id, position } => {
                (3u8, position).hash(h);
                hash_node(*id, styles, h);
            }
            Block::Image {
                id,
                url,
                alt,
                position,
            } => {
                (4u8, url, alt, position).hash(h);
                hash_node(*id, styles, h);
            }
        }
    }
}

fn hash_inlines(inlines: &[Inline], styles: &StyleTree, h: &mut DefaultHasher) {
    for inline in inlines {
        match inline {
            Inline::Text {
                id,
                value,
                position,
            } => {
                (0u8, value, position).hash(h);
                hash_node(*id, styles, h);
            }
            Inline::Emphasis {
                id,
                children,
                position,
            } => {
                (1u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(children, styles, h);
            }
            Inline::Strong {
                id,
                children,
                position,
            } => {
                (2u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(children, styles, h);
            }
            Inline::Code {
                id,
                value,
                position,
            } => {
                (3u8, value, position).hash(h);
                hash_node(*id, styles, h);
            }
            Inline::Link {
                id,
                url,
                children,
                position,
            } => {
                (4u8, url, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(children, styles, h);
            }
        }
    }
}

/// One node's resolved styling, the initial letter beside it.
fn hash_node(id: NodeId, styles: &StyleTree, h: &mut DefaultHasher) {
    hash_layout(styles.style(id), h);
    match styles.first_letter(id) {
        Some(style) => {
            1u8.hash(h);
            hash_layout(style, h);
        }
        None => 0u8.hash(h),
    }
}

/// Everything a fragment is built from. Destructured field by field
/// on purpose: a property added to `ComputedStyle` stops compiling
/// here until somebody says which stage it belongs to.
fn hash_layout(style: &ComputedStyle, h: &mut DefaultHasher) {
    let ComputedStyle {
        font_id,
        // The families asked for chose the face; layout reads only
        // the answer.
        font_family: _,
        font_size,
        font_style: _,
        font_weight: _,
        // No line moves for it. The runs the broken lines hold
        // carry the colour, so a sheet that recolours them has to
        // break them again.
        color,
        line_height,
        letter_spacing,
        font_variant_caps,
        text_transform,
        text_align,
        text_justify,
        hanging_punctuation,
        text_indent,
        hyphens,
        orphans,
        widows,
        // The named page is settled when a page opens, not when a
        // line breaks.
        page: _,
        content,
        string_set,
        counter_reset,
        initial_letter,
        margin,
        break_before,
        break_after,
        break_inside,
    } = style;
    (font_id, font_size.to_bits(), line_height.to_bits(), color).hash(h);
    (letter_spacing.to_bits(), font_variant_caps, text_transform).hash(h);
    (text_align, text_justify, hanging_punctuation).hash(h);
    (text_indent.to_bits(), hyphens, orphans, widows).hash(h);
    (content, string_set, counter_reset, initial_letter).hash(h);
    (break_before, break_after, break_inside).hash(h);
    hash_edges(*margin, h);
}

fn hash_geometry(geometry: PageGeometry, h: &mut DefaultHasher) {
    let PageGeometry {
        width,
        height,
        margin,
    } = geometry;
    (width.to_bits(), height.to_bits()).hash(h);
    hash_edges(margin, h);
}

fn hash_edges(edges: Edges, h: &mut DefaultHasher) {
    let Edges {
        top,
        right,
        bottom,
        left,
    } = edges;
    [top, right, bottom, left].map(f32::to_bits).hash(h);
}

fn hash_nodes(styles: &StyleTree, h: &mut DefaultHasher) {
    for node in styles.nodes() {
        (node.id, node.element, node.style, node.first_letter).hash(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{HeadingLevel, Metadata};
    use crate::pages::DrawItem;
    use crate::style::{Color, CounterStyle, Source};

    fn registry() -> &'static FontRegistry {
        static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
        REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
    }

    /// The fixture map: a JPEG the layout pass sizes from its header.
    const MAP: &[u8] = include_bytes!("../../../fixtures/images/plate.jpg");

    /// An image the host pushes reaches the display structure: the box is
    /// placed, the asset table names the url, and the pages are
    /// broken again around the room it takes.
    #[test]
    fn an_image_pushed_after_the_book_is_placed_and_indexed() {
        let mut book = Book {
            sections: vec![Section {
                blocks: vec![Block::Image {
                    id: NodeId::UNASSIGNED,
                    url: "plate.jpg".into(),
                    alt: "a map".into(),
                    position: None,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        book.assign_node_ids();
        let mut session =
            Session::owning(crate::fonts::bundled_registry().expect("bundled font parses"));
        session.set_content(book);
        assert!(
            session.preview().assets.is_empty(),
            "a book whose images nobody supplied has no assets",
        );

        assert_eq!(
            session.add_image("plate.jpg", MAP.to_vec()).unwrap(),
            Some(0),
        );
        let output = session.preview();
        assert_eq!(output.assets.len(), 1);
        assert_eq!(output.assets[0].url, "plate.jpg");
        let placed: Vec<&DrawItem> = output
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter(|item| matches!(item, DrawItem::Image { .. }))
            .collect();
        assert_eq!(placed.len(), 1, "the map was not placed");
        assert!(
            output.warnings.is_empty(),
            "a supplied image still complains: {:?}",
            output.warnings,
        );
    }

    /// A session that borrowed its asset table says so rather than
    /// adding to somebody else's.
    #[test]
    fn a_borrowed_asset_table_refuses_an_image() {
        let mut session = Session::new(registry());
        assert!(matches!(
            session.add_image("plate.jpg", MAP.to_vec()),
            Err(AddImageError::Borrowed),
        ));
    }

    fn text(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            position: None,
        }
    }

    fn paragraph(value: &str) -> Block {
        Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![text(value)],
            position: None,
        }
    }

    fn heading(value: &str) -> Block {
        Block::Heading {
            id: NodeId::UNASSIGNED,
            level: HeadingLevel::H1,
            inlines: vec![text(value)],
            position: None,
        }
    }

    /// Paragraphs of one word repeated, so that every line of a
    /// section repeats its tag and can be followed across an edit.
    fn prose(tag: &str, paragraphs: usize) -> Vec<Block> {
        (0..paragraphs)
            .map(|_| paragraph(&format!("{tag} ").repeat(80)))
            .collect()
    }

    fn section(source: &str, blocks: Vec<Block>) -> Section {
        Section {
            id: NodeId::UNASSIGNED,
            source: Some(source.into()),
            title: None,
            blocks,
            position: None,
        }
    }

    fn book(sections: Vec<Section>) -> Book {
        Book {
            metadata: Metadata::default(),
            sections,
        }
    }

    fn sheets(css: &str) -> Stylesheets {
        Stylesheets::parse(&[Source::author("test.css", css)])
    }

    /// A session over three chapters, one file each.
    fn three_chapters() -> Session<'static> {
        let mut session = Session::new(registry());
        session.set_content(book(vec![
            section("one.md", prose("alpha", 8)),
            section("two.md", prose("beta", 8)),
            section("three.md", prose("gamma", 8)),
        ]));
        session.preview();
        session
    }

    /// Naming a book costs nothing: no stage between the content
    /// tree and the page reads metadata, so the pages already laid
    /// out are the ones the export writes under the new name.
    #[test]
    fn naming_a_book_re_runs_no_stage() {
        let mut session = three_chapters();
        let before = session.stages();
        let pages = session.preview().pages.len();
        session.set_metadata(Metadata {
            title: Some("Gulliver's Travels".into()),
            author: Some("Jonathan Swift".into()),
            extra: [("language".to_string(), "en".to_string())]
                .into_iter()
                .collect(),
        });
        let after = session.preview().pages.len();
        assert_eq!(session.stages(), before);
        assert_eq!(after, pages);
        assert_eq!(
            session.book().metadata.title.as_deref(),
            Some("Gulliver's Travels")
        );
    }

    /// Every text run containing `tag`, as `(page, x, baseline, text)`.
    fn runs(output: &LayoutOutput, tag: &str) -> Vec<(usize, u32, u32, String)> {
        let mut found = Vec::new();
        for (index, page) in output.pages.iter().enumerate() {
            for item in &page.items {
                if let DrawItem::Text { x, y, text, .. } = item
                    && text.contains(tag)
                {
                    found.push((index, x.to_bits(), y.to_bits(), text.clone()));
                }
            }
        }
        found
    }

    fn spelled(runs: &[(usize, u32, u32, String)]) -> Vec<&str> {
        runs.iter().map(|(_, _, _, text)| text.as_str()).collect()
    }

    fn placed(runs: &[(usize, u32, u32, String)]) -> Vec<(usize, u32, u32)> {
        runs.iter().map(|(page, x, y, _)| (*page, *x, *y)).collect()
    }

    /// A `@page` change that leaves the measure where it was is
    /// answered by fragmentation: the lines it flows are the ones it
    /// already had.
    #[test]
    fn page_geometry_re_fragments_without_breaking_lines() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_style(sheets("@page { margin-bottom: 108pt }"));
        session.preview();
        let after = session.stages();
        assert_eq!(after.lines, before.lines, "the lines were broken again");
        assert_eq!(after.flow, before.flow + 1, "fragmentation did not run");
        assert_eq!(
            after.paint,
            before.paint + 1,
            "the furniture was not painted"
        );
    }

    /// The margin boxes are shallower still: the pages are settled,
    /// and only their furniture is painted again.
    #[test]
    fn a_running_foot_repaints_over_settled_pages() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_style(sheets("@page { @bottom-center { content: \"leaf\" } }"));
        let footed = !runs(session.preview(), "leaf").is_empty();
        let after = session.stages();
        assert!(footed, "the new foot was never painted");
        assert_eq!(after.lines, before.lines, "the lines were broken again");
        assert_eq!(after.flow, before.flow, "the pages were fragmented again");
        assert_eq!(
            after.paint,
            before.paint + 1,
            "the furniture was not painted"
        );
    }

    /// A page repainted twice ends up with one set of furniture,
    /// because the paint discards what the last one left rather than
    /// stacking on it.
    #[test]
    fn repainting_does_not_stack_furniture() {
        let mut session = three_chapters();
        let once = session.preview().pages[1].items.len();
        session.set_style(sheets(
            "@page { @bottom-center { content: counter(page) } }",
        ));
        let twice = session.preview().pages[1].items.len();
        assert_eq!(once, twice, "the second folio was painted over the first");
    }

    /// A change the engine models nothing of costs nothing. The
    /// display structure is served back as it stands, and only the
    /// diagnostics move.
    #[test]
    fn an_unsupported_property_runs_no_stage() {
        let mut session = three_chapters();
        let before = session.stages();
        let painted = serde_json::to_vec(&session.preview().pages).expect("pages serialize");

        session.set_style(sheets("p { background-color: rebeccapurple }"));
        let (repainted, complained) = {
            let output = session.preview();
            (
                serde_json::to_vec(&output.pages).expect("pages serialize"),
                output
                    .warnings
                    .iter()
                    .any(|warning| warning.message.contains("`background-color`")),
            )
        };
        let after = session.stages();

        assert_eq!(repainted, painted, "the display structure changed");
        assert!(complained, "the unsupported property went unreported");
        assert_eq!(after.lines, before.lines, "the lines were broken again");
        assert_eq!(after.flow, before.flow, "the pages were fragmented again");
        assert_eq!(after.paint, before.paint, "the furniture was painted again");
    }

    /// A colour edit breaks the lines again: the runs the broken
    /// lines hold are what carry the colour.
    #[test]
    fn a_colour_edit_breaks_the_lines_again() {
        let mut session = three_chapters();
        let before = session.stages();
        session.preview();

        session.set_style(sheets("p { color: rebeccapurple }"));
        let coloured = session
            .preview()
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .any(|item| {
                matches!(item, DrawItem::Text { color, .. } if *color == Color::rgb(102, 51, 153))
            });
        assert!(coloured, "the colour did not reach the page");
        assert!(
            session.stages().lines > before.lines,
            "the lines were served from the break cache in the old colour"
        );
    }

    /// The replaceable unit is the file: a host names one, and only
    /// the sections that came from it are broken again.
    #[test]
    fn replacing_a_source_re_breaks_only_its_own_sections() {
        let mut session = three_chapters();
        assert_eq!(
            session.stages().lines,
            3,
            "the first pass broke every section"
        );
        session.replace_source("two.md", vec![section("two.md", prose("delta", 9))]);
        session.preview();
        assert_eq!(
            session.stages().lines,
            4,
            "a section other than two.md was broken again"
        );
    }

    /// One file may split into several sections, and all of them go
    /// when it is replaced.
    #[test]
    fn a_source_that_split_into_several_sections_is_replaced_whole() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![
            section("one.md", vec![heading("One"), paragraph("first")]),
            section("one.md", vec![heading("Two"), paragraph("second")]),
            section("two.md", vec![heading("Three"), paragraph("third")]),
        ]));
        session.preview();
        session.replace_source("one.md", vec![section("one.md", vec![heading("Only")])]);
        session.preview();
        let sources: Vec<Option<&str>> = session
            .book()
            .sections
            .iter()
            .map(|section| section.source.as_deref())
            .collect();
        assert_eq!(sources, vec![Some("one.md"), Some("two.md")]);
        assert_eq!(session.book().sections.len(), 2);
    }

    /// A file the book has not seen before arrives at the end.
    #[test]
    fn a_source_the_book_does_not_carry_is_appended() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![section("one.md", vec![paragraph("first")])]));
        session.replace_source("two.md", vec![section("two.md", vec![paragraph("second")])]);
        session.preview();
        assert_eq!(session.book().sections.len(), 2);
    }

    /// The cache stores breaks and no positions, so a section the
    /// flow moved paints at new coordinates with the same lines.
    #[test]
    fn a_moved_section_keeps_its_breaks_and_takes_new_coordinates() {
        let mut session = Session::new(registry());
        // Chapters that open where the last one ended, so an edit
        // above moves what follows instead of leaving it on its own
        // opening page.
        session.set_style(sheets("section { break-before: auto }"));
        session.set_content(book(vec![
            section("one.md", prose("alpha", 2)),
            section("two.md", prose("beta", 12)),
        ]));
        let before = runs(session.preview(), "beta");
        let broke = session.stages().lines;

        session.replace_source("one.md", vec![section("one.md", prose("alpha", 24))]);
        let after = runs(session.preview(), "beta");

        assert_eq!(
            session.stages().lines,
            broke + 1,
            "the untouched section was broken again"
        );
        assert!(!before.is_empty(), "the section painted nothing");
        assert_eq!(spelled(&before), spelled(&after), "the breaks moved");
        assert_ne!(placed(&before), placed(&after), "nothing moved");
    }

    /// Mirrored margins are two masters of one measure, which is the
    /// book the built-in sheet sets and the case the cache is for.
    #[test]
    fn mirrored_margins_are_still_one_measure() {
        let session = three_chapters();
        assert!(session.reuses_sections());
    }

    /// Masters of different measures make where a line breaks depend
    /// on which page it lands on. The session stops reusing lines
    /// rather than serve breaks taken against another page's measure.
    #[test]
    fn asymmetric_page_margins_fall_back_to_full_re_breaking() {
        let mut session = three_chapters();
        session.set_style(sheets("@page :left { margin-left: 20pt }"));
        session.preview();
        assert!(
            !session.reuses_sections(),
            "two measures, and the cache still claims to be sound"
        );

        let broke = session.stages().lines;
        session.replace_source("two.md", vec![section("two.md", prose("delta", 9))]);
        session.preview();
        assert_eq!(
            session.stages().lines,
            broke + 3,
            "a section kept lines broken against a measure it may not land on"
        );
    }

    /// Inline text that depended on pagination would make breaking
    /// depend on where the breaks fell. The parser keeps the page
    /// counter out of element rules, so prose cannot ask for it, and
    /// the precondition says so out loud rather than leaving it to
    /// somebody's memory.
    #[test]
    fn generated_content_that_depends_on_pagination_closes_the_cache() {
        let plain = ComputedStyle::initial();
        let ornament = ComputedStyle {
            content: Content::Text("\u{2766}".into()),
            ..plain.clone()
        };
        assert!(!paginated_prose(&[plain.clone(), ornament]));

        let folio = ComputedStyle {
            content: Content::Counter(CounterStyle::Decimal),
            ..plain.clone()
        };
        assert!(paginated_prose(&[plain.clone(), folio]));

        let running = ComputedStyle {
            content: Content::String("chapter".into()),
            ..plain
        };
        assert!(paginated_prose(&[running]));
    }

    /// A sheet that asks for it anyway gets a diagnostic rather than
    /// a stale cache, and the ornament keeps whatever it had.
    #[test]
    fn the_page_counter_never_reaches_prose() {
        let mut session = three_chapters();
        session.set_style(sheets("hr { content: counter(page) }"));
        let complained = session
            .preview()
            .warnings
            .iter()
            .any(|warning| warning.message.contains("`content`"));
        assert!(complained, "the unsupported value went unreported");
        assert!(
            session.reuses_sections(),
            "prose the parser rejected still closed the cache"
        );
    }

    /// An export costs nothing the preview has not already paid: the
    /// stages above the painter are the same ones.
    #[test]
    fn export_paints_from_the_stages_the_preview_used() {
        let mut session = three_chapters();
        let before = session.stages();
        let bytes = session.export().expect("the fixture book writes PDF");
        assert_eq!(session.stages(), before, "an export re-ran a stage");
        assert!(bytes.starts_with(b"%PDF"), "that is not a PDF");
    }

    /// Setting the same content twice re-keys every section and
    /// breaks none of them: the key is what the section says, not
    /// which node ids it was given this time.
    #[test]
    fn identical_content_set_again_breaks_nothing() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_content(book(vec![
            section("one.md", prose("alpha", 8)),
            section("two.md", prose("beta", 8)),
            section("three.md", prose("gamma", 8)),
        ]));
        session.preview();
        assert_eq!(
            session.stages().lines,
            before.lines,
            "renumbering alone cost a re-break"
        );
    }

    /// A session that owns its registry takes a face after it was
    /// made, and lays out against it: bytes cross once, and the
    /// session they crossed into is the one that keeps them.
    #[test]
    fn an_owning_session_takes_a_face_and_uses_it() {
        let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
        session.set_content(book(vec![section("one.md", prose("alpha", 2))]));
        let faces = session.preview().fonts.len();

        let mut source = crate::fonts::FontSource::from_bytes(crate::fonts::BUNDLED_FONT.to_vec())
            .expect("the bundled face parses");
        source.family = "borrowed garamond".into();
        source.declared = Some(crate::fonts::FaceAttributes::REGULAR);
        let ids = session
            .add_font(source)
            .expect("an owning session registers");
        assert_eq!(ids.len(), 1);

        let css = "book { font-family: 'borrowed garamond' }";
        session.set_style(Stylesheets::parse(&[Source::author("faces.css", css)]));
        let output = session.preview();
        assert_eq!(
            output.fonts.len(),
            faces + 1,
            "the font table did not grow with the registry"
        );
        assert!(
            output
                .pages
                .iter()
                .flat_map(|page| &page.items)
                .any(|item| matches!(
                    item,
                    DrawItem::Text { font_id, .. } if *font_id == ids[0]
                )),
            "the face that was registered set nothing"
        );
    }

    /// A session laying out against someone else's registry has none
    /// of its own to add to, and says so rather than laying out
    /// against a face that is not there.
    #[test]
    fn a_borrowed_registry_refuses_a_face() {
        let mut session = Session::new(registry());
        let source = crate::fonts::FontSource::from_bytes(crate::fonts::BUNDLED_FONT.to_vec())
            .expect("the bundled face parses");
        assert!(matches!(
            session.add_font(source),
            Err(AddFontError::Borrowed)
        ));
    }

    /// A section that moves in the book keeps its lines: the key
    /// travels with the content, and nothing in it is positional.
    #[test]
    fn reordering_the_book_breaks_nothing() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_content(book(vec![
            section("three.md", prose("gamma", 8)),
            section("one.md", prose("alpha", 8)),
            section("two.md", prose("beta", 8)),
        ]));
        session.preview();
        assert_eq!(
            session.stages().lines,
            before.lines,
            "a section that only moved was broken again"
        );
    }
}
