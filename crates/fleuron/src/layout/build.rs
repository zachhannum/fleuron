//! Blocks in, fragments out: margins folded together, breaks
//! settled, lines broken, the initial letter set beside them.

use std::borrow::Cow;
use std::sync::Arc;

use crate::content::{
    Block, GeneratedBox, Inline, NodeId, Section, SourcePos, block_attributes, block_id,
    block_position, inline_attributes, inline_id, origin, text,
};
use crate::lines::{Line, LineBreakOptions, Measure, Opening, Patterns, Shaped, Span};
use crate::pages::{DrawItem, PageBox};
use crate::style::{
    Break, ColumnSpan, ComputedStyle, Content, Hyphens, Position, StringPiece, StyleTree,
    TextAlign, TextJustify,
};

use super::Paginator;
use super::cap::Cap;
use super::flow::Painted;
use super::fragment::{
    BreakPoint, Decoration, Decorations, DropCap, Fragment, Marks, Piece, decoration,
};
use super::image::ImageSize;
use super::reference::Referring;

impl Paginator<'_> {
    /// One section's blocks as fragments, in document order:
    /// everything measurement decides, and nothing pagination does.
    ///
    /// A block of the section that spans the columns breaks to the
    /// whole content box, and one that does not breaks to a column. A
    /// spanning block moves to the next page whole rather than split
    /// under the columns above it.
    pub fn section_fragments(&self, section: &Section) -> Vec<Fragment> {
        let geometry = self.styles.default_page().geometry;
        let measure = geometry.measure();
        let mut builder = Builder::new(self, section.source.as_deref());
        let style = self.styles.style(section.id).clone();
        builder.name(section.id);
        let start = builder.open(section.id, &style, &[], 0.0, measure);
        let column = style.content_box(0.0, measure);
        let whole = style.content_box(0.0, geometry.content_size().0);
        for child in children(self.styles, section.id, &section.blocks, section.position) {
            builder.spanning = self.styles.style(child.id()).column_span == ColumnSpan::All;
            let (x, measure) = if builder.spanning { whole } else { column };
            let first = builder.fragments.len();
            builder.blocks([child], x, measure);
            if builder.spanning {
                for fragment in builder.fragments[first..]
                    .iter_mut()
                    .filter(|fragment| !matches!(fragment.piece, Piece::Anchor(_)))
                    .skip(1)
                {
                    fragment.break_before = BreakPoint::Forbidden;
                }
            }
        }
        builder.spanning = false;
        builder.close(&style, start);
        builder.fragments
    }
}

/// Builds one section's fragments: blocks in, everything the flow
/// needs to place them out.
pub(super) struct Builder<'a, 'p> {
    pub(super) paginator: &'p Paginator<'a>,
    /// The file the section was read from, for diagnostics.
    pub(super) source: Option<&'p str>,
    pub(super) fragments: Vec<Fragment>,
    /// What the cascade has asked for above the next fragment.
    pub(super) pending: BreakPoint,
    /// The collapsible margin standing above the next fragment,
    /// which is the larger of the margins that met there.
    pub(super) margin: f32,
    /// Space above the next fragment that no margin collapses
    /// through: the borders and padding the blocks around it set.
    pub(super) fixed: f32,
    /// What the blocks opened so far have set, waiting for a fragment
    /// to attach it to a page.
    pub(super) pending_marks: Option<Box<Marks>>,
    /// The blocks still open, outermost first.
    open: Vec<Pending>,
    /// Whether the block being built spans every column.
    spanning: bool,
    /// The layer the block being built paints in. Every block opens
    /// before it emits anything, so opening one is where this is
    /// settled.
    pub(super) layer: i32,
    /// The sum of the moves of every relative block still open around
    /// the block being built.
    offset: (f32, f32),
    /// The value of `offset` before each open relative block added its
    /// move, innermost last.
    moved: Vec<(f32, f32)>,
    /// How much shows of what the block being built paints, from 0 to
    /// 1: the `opacity` of every block still open around it, its own
    /// included, multiplied together.
    pub(super) opacity: f32,
    /// The value of `opacity` before each open block multiplied its
    /// own into it, innermost last.
    faded: Vec<f32>,
    /// The block this builder lays out on its own, against the page.
    /// Its own `position: absolute` does not anchor it a second time.
    pub(super) lifted: Option<NodeId>,
    /// How far down the fragments emitted so far reach, space above
    /// each one included.
    depth: f32,
    /// The blocks still open that ask for a height, innermost last.
    tall: Vec<Tall>,
}

impl<'a, 'p> Builder<'a, 'p> {
    /// A builder with nothing built yet, over blocks read from
    /// `source`.
    pub(super) fn new(paginator: &'p Paginator<'a>, source: Option<&'p str>) -> Builder<'a, 'p> {
        Builder {
            paginator,
            source,
            fragments: Vec::new(),
            pending: BreakPoint::Allowed,
            margin: 0.0,
            fixed: 0.0,
            pending_marks: None,
            open: Vec::new(),
            spanning: false,
            layer: 0,
            offset: (0.0, 0.0),
            moved: Vec::new(),
            opacity: 1.0,
            faded: Vec::new(),
            lifted: None,
            depth: 0.0,
            tall: Vec::new(),
        }
    }
}

/// One box among the children of an element: a block the manuscript
/// wrote, or a box that `::before` or `::after` generates.
#[derive(Debug, Clone, Copy)]
pub(super) enum Child<'b> {
    Block(&'b Block),
    /// A generated box, and where its element was written.
    Generated(NodeId, Option<SourcePos>),
}

impl Child<'_> {
    pub(super) fn id(self) -> NodeId {
        match self {
            Child::Block(block) => block_id(block),
            Child::Generated(id, _) => id,
        }
    }
}

/// The children of the element `id`, written at `position`: the box
/// `::before` generates, then `blocks`, then the box `::after`
/// generates.
pub(super) fn children<'b>(
    styles: &StyleTree,
    id: NodeId,
    blocks: &'b [Block],
    position: Option<SourcePos>,
) -> impl Iterator<Item = Child<'b>> + use<'b> {
    let generated = |which| {
        styles
            .generated_box(id, which)
            .map(|id| Child::Generated(id, position))
    };
    generated(GeneratedBox::Before)
        .into_iter()
        .chain(blocks.iter().map(Child::Block))
        .chain(generated(GeneratedBox::After))
}

/// A block that asks for a height, while its fragments are still being
/// built.
struct Tall {
    /// Where its content box starts, as `depth` counts.
    top: f32,
    /// The height `height` gives its content box, which is what a
    /// percentage inside it measures against. `None` for `auto`.
    definite: Option<f32>,
    /// The least height its content box takes.
    least: f32,
}

/// A block while its fragments are still being built.
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
    pub(super) fn ask(&mut self, wanted: Break) {
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
    pub(super) fn open(
        &mut self,
        node: NodeId,
        style: &ComputedStyle,
        inlines: &[Inline],
        x: f32,
        measure: f32,
    ) -> usize {
        self.ask(style.break_before);
        self.mark(style, inlines);
        self.layer = style.z_index;
        if style.position == Position::Relative {
            // A percentage is a percentage of the page area that the
            // lines of the section break to.
            let area = self.styles().default_page().geometry.content_size();
            let (dx, dy) = style.inset.offset(area);
            self.moved.push(self.offset);
            self.offset = (self.offset.0 + dx, self.offset.1 + dy);
        }
        self.faded.push(self.opacity);
        self.opacity *= style.opacity;
        self.margin = self.margin.max(style.margin.top);
        let start = self.fragments.len();
        let border = style.border.widths();
        let backdrop = self.paginator.backdrop(&style.background);
        self.open.push(Pending {
            start,
            open_fixed: self.fixed,
            started: false,
            decoration: decoration(
                node,
                style,
                x,
                measure,
                backdrop,
                (self.offset, self.opacity),
            ),
        });
        if border.top + style.padding.top > 0.0 {
            let margin = std::mem::take(&mut self.margin);
            self.commit(margin);
            self.fixed += border.top + style.padding.top;
        }
        if style.sized() {
            let within = self.tall_within();
            let definite = style.height.resolve(within);
            let least = style
                .min_height
                .resolve(within)
                .unwrap_or(0.0)
                .max(definite.unwrap_or(0.0));
            self.tall.push(Tall {
                top: self.depth + self.fixed + self.margin,
                definite,
                least,
            });
        }
        start
    }

    /// The height a percentage height measures against: the nearest
    /// definite `height` around the block, or the page's content box.
    fn tall_within(&self) -> f32 {
        self.tall
            .iter()
            .rev()
            .find_map(|tall| tall.definite)
            .unwrap_or_else(|| self.styles().default_page().geometry.content_size().1)
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

    /// Records a node a link can reach. It lands on the first fragment
    /// the node emits, the way a string it sets does, so the page that
    /// fragment lands on is the page a reference to the node prints.
    pub(super) fn name(&mut self, node: NodeId) {
        let marks = self.pending_marks.get_or_insert_with(Box::default);
        marks.targets.push(node);
    }

    /// The same for the inlines of one block that carry an id, which
    /// land on the block's first line.
    fn name_inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            if inline_attributes(inline).id.is_some() {
                self.name(inline_id(inline));
            }
            if let Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Link { children, .. } = inline
            {
                self.name_inlines(children);
            }
        }
    }

    /// Closes a block: `break-inside: avoid` glues everything it
    /// emitted, its bottom border and padding take height of their
    /// own, and its bottom margin becomes the next block's lead.
    pub(super) fn close(&mut self, style: &ComputedStyle, start: usize) {
        if style.sized() {
            let tall = self.tall.pop().expect("the block opened a height");
            let rest = tall.least - (self.depth + self.fixed + self.margin - tall.top);
            if rest > 0.0 {
                self.remainder(rest, start);
            }
        }
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
        let pending = self.open.pop().expect("the block opened a box");
        self.seal(pending);
        if style.position == Position::Relative
            && let Some(offset) = self.moved.pop()
        {
            self.offset = offset;
        }
        if let Some(opacity) = self.faded.pop() {
            self.opacity = opacity;
        }
        self.margin = self.margin.max(style.margin.bottom);
        // A block that emitted nothing settles nothing: what was
        // asked above it is still asked above whatever comes next.
        if self.fragments.len() > start {
            self.pending = BreakPoint::Allowed;
        }
        self.ask(style.break_after);
    }

    /// The space a block taller than its content leaves below that
    /// content, as a fragment of the block. A page does not end
    /// between the content and the space. A block that emitted nothing
    /// else takes what was asked above it here.
    fn remainder(&mut self, rest: f32, start: usize) {
        let mut blank = Fragment::plain(0.0, rest, Piece::Blank);
        let mut first = self.fragments[start..]
            .iter()
            .all(|fragment| matches!(fragment.piece, Piece::Anchor(_)));
        if !first {
            blank.break_before = BreakPoint::Forbidden;
            blank.lead = std::mem::take(&mut self.margin);
            blank.fixed = std::mem::take(&mut self.fixed);
        }
        self.emit(&mut first, blank);
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
        self.emit(&mut true, Fragment::plain(x, height, piece));
    }

    /// Emits one fragment. The first of a block gets the break the
    /// cascade asked for above it and the space its margins left; the
    /// rest keep what they arrived with, which is what the block says
    /// about splitting itself.
    pub(super) fn emit(&mut self, first: &mut bool, mut fragment: Fragment) {
        // This fragment settles how far the border box of every
        // block opening on it sits above it.
        let (index, fixed) = (self.fragments.len(), self.fixed);
        for pending in &mut self.open {
            if pending.start == index {
                pending.decoration.above = fixed - pending.open_fixed;
            }
        }
        if *first {
            *first = false;
            fragment.break_before = std::mem::replace(&mut self.pending, BreakPoint::Allowed);
            fragment.lead = std::mem::take(&mut self.margin);
            fragment.fixed = std::mem::take(&mut self.fixed);
            fragment.marks = self.pending_marks.take();
        }
        fragment.spanning = self.spanning;
        fragment.layer = self.layer;
        fragment.offset = self.offset;
        fragment.opacity = self.opacity;
        self.depth += fragment.lead + fragment.fixed + fragment.height;
        self.fragments.push(fragment);
    }

    /// Marks where an image the sheet lifted out of the flow was
    /// written. The fragment takes no space. The page the flow
    /// reaches when it passes here is the page that carries the
    /// image.
    pub(super) fn anchor(&mut self, id: NodeId) {
        self.fragments
            .push(Fragment::plain(0.0, 0.0, Piece::Anchor(id)));
    }

    /// Everything built so far, stacked in a box that no page break
    /// splits. The result holds what the blocks paint, from the top
    /// and the leading edge of that box, and the height of the box.
    ///
    /// A table cell is one of these, and so is a block anchored to
    /// the page.
    pub(super) fn stack(mut self) -> Stacked {
        let mut marks = None;
        let mut anchors = Vec::new();
        let mut placed = Vec::new();
        let mut cursor = 0.0f32;
        for fragment in &self.fragments {
            if let Piece::Anchor(node) = fragment.piece {
                anchors.push(node);
                continue;
            }
            gather(&mut marks, fragment.marks.clone());
            let top = cursor + fragment.lead + fragment.fixed;
            placed.push((top, fragment));
            cursor = top + fragment.height;
        }
        gather(&mut marks, self.pending_marks.take());
        let (mut items, boxes) = decorate(&placed);
        for (top, fragment) in &placed {
            items.append(&mut self.paginator.fragment_items(fragment, 0.0, *top));
        }
        Stacked {
            items,
            boxes,
            height: cursor + self.margin + self.fixed,
            anchors,
            marks,
        }
    }

    /// Every block of one nesting level, at `x` from the content
    /// box's leading edge and breaking to `measure`.
    pub(super) fn blocks<'b>(
        &mut self,
        boxes: impl IntoIterator<Item = Child<'b>>,
        x: f32,
        measure: f32,
    ) {
        for child in boxes {
            let id = child.id();
            if let Child::Block(block) = child
                && (matches!(block, Block::Heading { .. }) || block_attributes(block).id.is_some())
            {
                self.name(id);
            }
            // A box against the page is not in the flow: it takes no
            // space here, and the margins that met around it still
            // meet.
            if self.lifted != Some(id) && self.styles().style(id).position == Position::Absolute {
                self.anchor(id);
                continue;
            }
            let block = match child {
                Child::Block(block) => block,
                Child::Generated(id, position) => {
                    self.generated(id, position, x, measure);
                    continue;
                }
            };
            let styles = self.paginator.styles;
            let position = block_position(block);
            match block {
                Block::Heading { id, inlines, .. } | Block::Paragraph { id, inlines, .. } => {
                    self.name_inlines(inlines);
                    self.paragraph(*id, inlines, position, x, measure);
                }
                Block::Blockquote { id, blocks, .. } => {
                    let style = styles.style(*id).clone();
                    let start = self.open(*id, &style, &[], x, measure);
                    let (inner, narrowed) = style.content_box(x, measure);
                    self.blocks(children(styles, *id, blocks, position), inner, narrowed);
                    self.close(&style, start);
                }
                Block::ThematicBreak { id, .. } => {
                    let style = styles.style(*id).clone();
                    let start = self.open(*id, &style, &[], x, measure);
                    let (inner, narrowed) = style.content_box(x, measure);
                    self.pseudo(*id, GeneratedBox::Before, position, inner, narrowed);
                    self.ornament(&style, x, measure);
                    self.pseudo(*id, GeneratedBox::After, position, inner, narrowed);
                    self.close(&style, start);
                }
                Block::Image { id, url, .. } => {
                    let style = styles.style(*id).clone();
                    // The image takes its `height` itself. A scaled
                    // image would otherwise leave the rest below it.
                    let boxed = ComputedStyle {
                        height: crate::style::Width::Auto,
                        ..style.clone()
                    };
                    let start = self.open(*id, &boxed, &[], x, measure);
                    let (inner, narrowed) = style.content_box(x, measure);
                    self.pseudo(*id, GeneratedBox::Before, position, inner, narrowed);
                    self.image(&style, url, origin(self.source, position), x, measure);
                    self.pseudo(*id, GeneratedBox::After, position, inner, narrowed);
                    self.close(&boxed, start);
                }
                Block::List {
                    id,
                    start,
                    items,
                    position,
                    ..
                } => self.list(*id, *start, items, *position, x, measure),
                Block::Table {
                    id,
                    head,
                    body,
                    position,
                    ..
                } => self.table(*id, head, body, *position, x, measure),
            }
        }
    }

    /// One paragraph's lines as fragments: alignment against each
    /// band, the initial letter beside the first of them, and where a
    /// page can end between them.
    ///
    /// `spec` is the profile the lines were broken to, and `gaps` the
    /// space above a band the profile had to move past an image.
    fn paragraph(
        &mut self,
        id: NodeId,
        inlines: &[Inline],
        position: Option<SourcePos>,
        x: f32,
        measure: f32,
    ) {
        let computed = self.styles().style(id).clone();
        let start = self.open(id, &computed, inlines, x, measure);
        let (x, measure) = computed.content_box(x, measure);
        self.pseudo(id, GeneratedBox::Before, position, x, measure);

        let style = computed.paragraph();
        let options = self.options(&computed);
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
        let referring = Referring {
            paginator: self.paginator,
            source: self.source,
        };
        let (broken, shaped) = self
            .paginator
            .lines
            .layout_shaped(inlines, style, &referring, &spec, options, opening);

        let setting = Setting {
            x,
            align: computed.text_align,
            orphans: computed.orphans as usize,
            widows: computed.widows as usize,
            cap: cap.map(|(cap, _)| cap),
            cap_x: 0.0,
        };
        let fragments = set_lines(self.paginator, broken.lines, &spec, &[], &setting);
        let reflow = shaped.filter(|_| self.paginator.wraps()).map(|shaped| {
            Arc::new(Reflow {
                leading: self.paginator.lines.strut(style).height(),
                shaped,
                setting,
                base: spec,
                ends: broken.ends,
            })
        });
        let mut first = true;
        for mut fragment in fragments {
            fragment.reflow = reflow.clone();
            self.emit(&mut first, fragment);
        }
        self.pseudo(id, GeneratedBox::After, position, x, measure);
        self.close(&computed, start);
    }

    /// How the lines of a block in `style` break.
    fn options(&self, style: &ComputedStyle) -> LineBreakOptions {
        let hyphenate = style.hyphens == Hyphens::Auto;
        LineBreakOptions {
            hyphenate,
            patterns: if hyphenate {
                self.paginator.patterns()
            } else {
                Patterns::NONE
            },
            justify: style.text_align == TextAlign::Justify,
            inter_character: style.text_justify == TextJustify::InterCharacter,
            hanging: style.hanging_punctuation,
        }
    }

    /// The children of the block `element` that `::before` or
    /// `::after` generates, at `x` and `measure`. `position` is where
    /// the block was written.
    pub(super) fn pseudo(
        &mut self,
        element: NodeId,
        which: GeneratedBox,
        position: Option<SourcePos>,
        x: f32,
        measure: f32,
    ) {
        let child = self
            .styles()
            .generated_box(element, which)
            .map(|id| Child::Generated(id, position));
        self.blocks(child, x, measure);
    }

    /// The generated box `id` in the flow, across the content box at
    /// `x` and `measure`. `position` is where its element was written.
    /// It holds the text of its `content`, or nothing. A page does not
    /// end between the box and the rest of its element.
    fn generated(&mut self, id: NodeId, position: Option<SourcePos>, x: f32, measure: f32) {
        let Some((_, which)) = id.generated_box() else {
            return;
        };
        let mut style = self.styles().style(id).clone();
        // A box with no layer of its own paints in the layer of its
        // block.
        let layer = self.layer;
        if style.z_index == 0 {
            style.z_index = layer;
        }
        if which == GeneratedBox::After {
            self.ask(Break::Avoid);
        }
        let start = self.open(id, &style, &[], x, measure);
        let (x, measure) = style.content_box(x, measure);
        let text = self
            .paginator
            .generate(&style.content, None, self.source, position)
            .unwrap_or_default();
        let spec = Measure::uniform(measure);
        let lines = if text.is_empty() {
            Vec::new()
        } else {
            let options = self.options(&style);
            self.paginator
                .lines
                .layout_generated(&text, id, style.paragraph(), &spec, options)
        };
        if lines.is_empty() {
            self.emit_one(x, 0.0, Piece::Blank);
        } else {
            let setting = Setting {
                x,
                align: style.text_align,
                orphans: style.orphans as usize,
                widows: style.widows as usize,
                cap: None,
                cap_x: 0.0,
            };
            let mut first = true;
            for fragment in set_lines(self.paginator, lines, &spec, &[], &setting) {
                self.emit(&mut first, fragment);
            }
        }
        self.close(&style, start);
        self.layer = layer;
        if which == GeneratedBox::Before {
            self.ask(Break::Avoid);
        }
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

    /// A block image, at the size its style and its file ask for,
    /// scaled down when that does not fit the page. A percentage width
    /// measures against `measure`, the width the image is laid out in.
    fn image(&mut self, style: &ComputedStyle, url: &str, origin: String, x: f32, measure: f32) {
        let Some((asset, intrinsic)) = self.paginator.assets.lookup(url) else {
            self.paginator.missing(url, origin);
            return;
        };
        let within = (measure, self.tall_within());
        let (x, measure) = style.content_box(x, measure);
        let available = self
            .paginator
            .styles
            .default_page()
            .geometry
            .content_size()
            .1;
        let ImageSize { width, height, .. } = self.paginator.image_size(
            style,
            url,
            intrinsic.size(),
            within,
            measure,
            available,
            (!origin.is_empty()).then_some(origin),
        );
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

/// One paragraph's lines as fragments: alignment against each band,
/// the initial letter beside the first of them, and where a page may
/// end between them.
///
/// `spec` is the profile the lines were broken to, and `gaps` the
/// space above a band the profile had to move past an image.
pub(super) fn set_lines(
    paginator: &Paginator,
    lines: Vec<Line>,
    spec: &Measure,
    gaps: &[f32],
    setting: &Setting,
) -> Vec<Fragment> {
    let count = lines.len();
    let sunk = setting
        .cap
        .as_ref()
        .map(|cap| cap.lines.min(count))
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
    let mut cap = setting.cap.as_ref().map(|cap| DropCap {
        line: cap.line.clone(),
        x: setting.x + setting.cap_x,
        drop,
    });

    let mut fragments = Vec::with_capacity(count);
    let mut slot = 0;
    for (index, mut line) in lines.into_iter().enumerate() {
        let last = line.spans.len() - 1;
        // A band's slack is at the end of its last span, so that
        // is where alignment moves the text.
        let offset = align_offset(
            setting.align,
            paginator.span_width(&line, last),
            spec.at(slot + last).width,
        );
        line.spans[last].offset += offset;
        let origin = spec.at(slot).origin;
        slot += line.spans.len();
        let height = line.box_.height;
        let protrusion = line.protrusion;
        let piece = Piece::Line {
            line,
            cap: (index == 0).then(|| cap.take()).flatten(),
        };
        let mut fragment = Fragment::plain(setting.x + origin - protrusion, height, piece);
        fragment.break_before =
            if index < setting.orphans || count - index < setting.widows || index < sunk {
                BreakPoint::Forbidden
            } else {
                BreakPoint::Allowed
            };
        fragment.fixed = gaps.get(index).copied().unwrap_or(0.0);
        fragments.push(fragment);
    }
    fragments
}

/// Moves what the flow reads off the fragments a paragraph arrived as
/// onto the ones it was set again as: the space and the break above
/// the first of them, what it tells the page furniture, and the
/// decorations that open and close over the paragraph.
///
/// `fixed` is the paragraph's own space above the first of them.
pub(super) fn carry_over(fresh: &mut [Fragment], old: &[Fragment], fixed: f32) {
    let (Some(head), Some(last)) = (old.first(), old.last()) else {
        return;
    };
    for fragment in fresh.iter_mut() {
        fragment.spanning = head.spanning;
        fragment.layer = head.layer;
        fragment.offset = head.offset;
        fragment.opacity = head.opacity;
    }
    let opens = head
        .decorations
        .as_ref()
        .map(|decorations| decorations.opens.clone())
        .unwrap_or_default();
    let closes = last
        .decorations
        .as_ref()
        .map(|decorations| decorations.closes)
        .unwrap_or(0);
    if let Some(first) = fresh.first_mut() {
        first.break_before = head.break_before;
        first.lead = head.lead;
        first.fixed += fixed;
        first.marks = head.marks.clone();
        first.markers = head.markers.clone();
        if !opens.is_empty() {
            first.decorations = Some(Box::new(Decorations {
                opens,
                ..Default::default()
            }));
        }
    }
    if let Some(end) = fresh.last_mut()
        && closes > 0
    {
        end.decorations.get_or_insert_with(Box::default).closes += closes;
    }
}

/// What one paragraph's lines are set against once they are broken:
/// where they start, how they fill a band, and what must not be split
/// from what.
#[derive(Debug, Clone)]
pub(super) struct Setting {
    /// Leading edge, from the content box's own.
    pub(super) x: f32,
    align: TextAlign,
    orphans: usize,
    widows: usize,
    /// The initial letter beside the lines it is sunk over.
    pub(super) cap: Option<Cap>,
    /// Where the letter goes, from `x`. An image in the way of the
    /// bands it is sunk over moves it along with them.
    cap_x: f32,
}

impl Setting {
    /// The same over the bands a profile left. An initial letter
    /// belongs to the line a paragraph opens on rather than to the
    /// line the rest of it opens on, and an image can move it.
    pub(super) fn wrapped(&self, opening: bool, letter: Option<f32>) -> Cow<'_, Setting> {
        if opening && letter.is_none() {
            return Cow::Borrowed(self);
        }
        Cow::Owned(Setting {
            cap: opening.then(|| self.cap.clone()).flatten(),
            cap_x: letter.unwrap_or(0.0),
            ..self.clone()
        })
    }
}

/// A paragraph the flow can set again.
///
/// The lines a section is built with are broken against the measure
/// with nothing in the way. A paragraph that lands beside an image is
/// broken again against the bands the image leaves. This is what that
/// takes: the shaped runs, which the measure has no say in, and
/// everything settled around them.
#[derive(Debug)]
pub struct Reflow {
    pub(super) shaped: Shaped,
    pub(super) setting: Setting,
    /// The bands with nothing in the way. A first-line indent and a
    /// drop cap are already in them.
    pub(super) base: Measure,
    /// Height of one band, which is what an image is snapped to.
    pub(super) leading: f32,
    /// Where each line ended, so the rest of the paragraph can be set
    /// from any of them.
    pub(super) ends: Vec<usize>,
}

/// What a stack of blocks comes to.
pub(super) struct Stacked {
    /// What they paint, from the top of their box.
    pub(super) items: Vec<DrawItem>,
    /// The border boxes of the blocks, from the top of their box.
    pub(super) boxes: Vec<(NodeId, PageBox)>,
    /// Their height, margins included.
    pub(super) height: f32,
    /// The boxes the sheet lifted out of the flow from inside them.
    pub(super) anchors: Vec<NodeId>,
    /// The strings, folios, and targets the blocks give the page
    /// furniture.
    pub(super) marks: Option<Box<Marks>>,
}

/// The decorated blocks inside one stack, as the rects they paint. A
/// stack is never split, so no box inside it is cut.
fn decorate(placed: &[(f32, &Fragment)]) -> (Vec<DrawItem>, Vec<(NodeId, PageBox)>) {
    let mut boxes: Vec<Painted> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for (top, fragment) in placed {
        let Some(decorations) = &fragment.decorations else {
            continue;
        };
        for decoration in &decorations.opens {
            open.push(boxes.len());
            boxes.push(Painted {
                top: top - decoration.above,
                decoration: decoration.clone(),
                bottom: 0.0,
                cut_above: false,
                cut_below: false,
            });
        }
        for _ in 0..decorations.closes {
            let Some(index) = open.pop() else { continue };
            boxes[index].bottom = top + fragment.height + boxes[index].decoration.below;
        }
    }
    let mut items = Vec::new();
    let mut areas = Vec::new();
    for painted in &boxes {
        let (x, y, width, height) = painted.border_box((0.0, 0.0));
        if height > 0.0 {
            let area = PageBox {
                page: 0,
                x,
                y,
                width,
                height,
            };
            areas.push((painted.decoration.node, area));
        }
        items.extend(painted.items((0.0, 0.0)));
    }
    (items, areas)
}

/// Adds the marks of one fragment to the marks gathered so far.
pub(super) fn gather(into: &mut Option<Box<Marks>>, from: Option<Box<Marks>>) {
    let Some(from) = from else {
        return;
    };
    let marks = into.get_or_insert_with(Box::default);
    marks.strings.extend(from.strings);
    marks.page_number = from.page_number.or(marks.page_number);
    marks.targets.extend(from.targets);
}

/// Where a line of `width` starts inside a measure of `available`.
fn align_offset(align: TextAlign, width: f32, available: f32) -> f32 {
    match align {
        TextAlign::Left | TextAlign::Justify => 0.0,
        TextAlign::Right => (available - width).max(0.0),
        TextAlign::Center => ((available - width) / 2.0).max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use crate::content::{Attributes, Block, HeadingLevel, NodeId, Section, SourcePos};
    use crate::layout::testing::{
        assert_orphans_and_widows, book_of, broken_heading, chapter_size, content_lines,
        folio_size, heading, long_prose, master, origin_of, ornament, paginate, paginate_styled,
        paragraph, png, prose, quote, rects, registry, right_edge, scene_break, section,
        small_caps_lines, styled, tagged_prose, ua, under_h3,
    };
    use crate::layout::{BreakPoint, Fragment, Paginator, Piece, layout_book};
    use crate::pages::{DrawItem, Page, Side};
    use crate::style::{Color, Situation};

    /// The baseline of the first line of the first page that opens
    /// with `token`.
    fn baseline_of(pages: &[Page], token: &str) -> f32 {
        content_lines(&pages[0])
            .into_iter()
            .find(|(_, runs)| runs[0].2.starts_with(token))
            .map(|(baseline, _)| baseline)
            .unwrap_or_else(|| panic!("no line opens with {token}"))
    }

    /// Acceptance: `h1 { height: 3in }` leaves three inches before the
    /// block under it, whatever the heading's own height.
    #[test]
    fn a_heading_with_a_height_leaves_that_height_before_the_block_under_it() {
        let alone = paginate(vec![section(vec![paragraph("under the heading")])]);
        let wanted = baseline_of(&alone, "under") + 216.0;
        for css in [
            "h1 { height: 3in }",
            "h1 { height: 3in; font-size: 40pt }",
            "h1 { height: 3in; font-size: 9pt }",
        ] {
            let pages = paginate_styled(
                css,
                vec![section(vec![
                    heading("Tall"),
                    paragraph("under the heading"),
                ])],
            );
            let under = baseline_of(&pages, "under");
            assert!(
                (under - wanted).abs() < 1e-3,
                "{css}: the paragraph sits on {under}, not {wanted}"
            );
        }
    }

    /// Acceptance: a percentage height resolves against the content box
    /// the block is in. For a block of the section that is the page's,
    /// and for a paragraph inside a quotation with a height it is the
    /// quotation's.
    #[test]
    fn a_percentage_height_resolves_against_the_content_box_the_block_is_in() {
        let (_, area) = ua().default_page().geometry.content_size();
        let alone = paginate(vec![section(vec![paragraph("after")])]);
        let top = baseline_of(&alone, "after");
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;

        let half = paginate_styled(
            "h1 { height: 50% }",
            vec![section(vec![heading("Half"), paragraph("after")])],
        );
        assert!(near(baseline_of(&half, "after"), top + area / 2.0));

        let nested = paginate_styled(
            "blockquote { height: 4in; margin: 0 } blockquote p { height: 50% }",
            vec![section(vec![
                quote(vec![paragraph("first"), paragraph("second")]),
                paragraph("after"),
            ])],
        );
        assert!(near(baseline_of(&nested, "first"), top));
        assert!(near(baseline_of(&nested, "second"), top + 144.0));
        assert!(near(baseline_of(&nested, "after"), top + 288.0));
    }

    /// Part: a block taller than its content leaves the remainder below
    /// the content, inside its own box. `min-height` does the same for
    /// a block whose content is shorter. A block whose content is
    /// taller than its height grows to hold it.
    #[test]
    fn a_block_taller_than_its_content_leaves_the_remainder_below_it() {
        use crate::layout::testing::rects;
        let sections = || {
            vec![section(vec![
                quote(vec![paragraph("quoted")]),
                paragraph("after"),
            ])]
        };
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let plain = paginate_styled("blockquote { margin: 0 }", sections());
        let quoted = baseline_of(&plain, "quoted");

        let tall = paginate_styled(
            "blockquote { margin: 0; height: 2in; background-color: #eeeeee }",
            sections(),
        );
        assert!(near(baseline_of(&tall, "quoted"), quoted));
        assert!(near(baseline_of(&tall, "after"), quoted + 144.0));
        let tint = rects(&tall[0]);
        assert_eq!(tint.len(), 1, "one box, one tint: {tint:?}");
        assert!(near(tint[0].3, 144.0), "the box is {} tall", tint[0].3);

        let least = paginate_styled("blockquote { margin: 0; min-height: 2in }", sections());
        assert!(near(baseline_of(&least, "after"), quoted + 144.0));

        let short = paginate_styled("blockquote { margin: 0; height: 1pt }", sections());
        assert_eq!(
            baseline_of(&short, "after"),
            baseline_of(&plain, "after"),
            "a block shorter than its content cut into it"
        );
    }

    /// A block that spans the columns breaks to the whole content
    /// box, and the prose around it breaks to one column. Past its
    /// first fragment it forbids every break, so it moves whole.
    #[test]
    fn a_spanning_block_breaks_to_the_content_box() {
        let css = "@page { column-count: 2; column-gap: 18pt } \
                   blockquote { column-span: all; margin: 0 }";
        let quoted = paragraph(&"a quiet sentence of prose ".repeat(18));
        let book = book_of(vec![section(vec![prose(), quote(vec![quoted]), prose()])]);
        let styles = styled(css, &book);
        let paginator = Paginator::new(registry(), &styles);
        let geometry = styles.default_page().geometry;
        let fragments = paginator.section_fragments(&book.sections[0]);
        let widest = |spanning: bool| {
            fragments
                .iter()
                .filter(|fragment| fragment.spanning == spanning)
                .filter_map(|fragment| match &fragment.piece {
                    Piece::Line { line, .. } => Some(fragment.x + paginator.line_width(line)),
                    _ => None,
                })
                .fold(0.0f32, f32::max)
        };
        assert!(widest(false) <= geometry.measure() + 1e-3);
        assert!(
            widest(true) > geometry.measure(),
            "the quotation broke to {} in a {} column",
            widest(true),
            geometry.measure(),
        );
        assert!(widest(true) <= geometry.content_size().0 + 1e-3);

        let spanning: Vec<&Fragment> = fragments.iter().filter(|f| f.spanning).collect();
        assert!(
            spanning.len() > 2,
            "the quotation set {} lines",
            spanning.len()
        );
        assert!(
            spanning[1..]
                .iter()
                .all(|fragment| fragment.break_before == BreakPoint::Forbidden)
        );
    }

    /// Acceptance: `h1 { position: relative; top: -12pt }` raises the
    /// heading and leaves the prose under it where it was.
    #[test]
    fn a_relative_heading_is_raised_and_the_prose_under_it_stays() {
        use crate::layout::testing::content_items;
        let sections = || vec![section([vec![heading("Raised")], long_prose(3)].concat())];
        let plain = paginate(sections());
        let raised = paginate_styled("h1 { position: relative; top: -12pt }", sections());
        assert_eq!(plain.len(), raised.len(), "the page count moved");
        let (plain, raised) = (content_items(&plain[0]), content_items(&raised[0]));
        assert_eq!(plain.len(), raised.len());
        let mut headings = 0;
        for (before, after) in plain.iter().zip(&raised) {
            assert_eq!(before.3, after.3, "the runs are painted in another order");
            assert_eq!(before.0, after.0, "{:?} moved across", after.3);
            if before.2 == chapter_size() {
                headings += 1;
                assert!(
                    (after.1 - (before.1 - 12.0)).abs() < 1e-3,
                    "the heading sits at {} rather than 12pt above {}",
                    after.1,
                    before.1,
                );
            } else {
                assert_eq!(before.1, after.1, "the prose {:?} moved", after.3);
            }
        }
        assert!(headings > 0, "no heading on the first page");
    }

    /// Part: the engine moves a relative block and its box on every
    /// page the block runs over. A percentage is a percentage of the
    /// page area. Nothing around the block moves, and the page breaks
    /// do not change.
    #[test]
    fn a_relative_block_moves_its_box_and_its_lines_on_every_page() {
        use crate::layout::testing::{content_items, rects};
        let sections = || {
            vec![section(
                [
                    long_prose(1),
                    vec![quote(vec![paragraph(&"lilliputian ".repeat(500))])],
                    long_prose(1),
                ]
                .concat(),
            )]
        };
        let base = "blockquote { background-color: #eeeeee; box-decoration-break: clone }";
        let plain = paginate_styled(base, sections());
        let moved = paginate_styled(
            &format!("{base} blockquote {{ position: relative; left: 10%; bottom: 6pt }}"),
            sections(),
        );
        let (dx, dy) = (ua().default_page().geometry.content_size().0 * 0.1, -6.0);
        assert!(plain.len() > 1, "the quotation fits on one page");
        assert_eq!(plain.len(), moved.len(), "the page count moved");
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        let (mut boxes, mut quoted) = (0, 0);
        for (before, after) in plain.iter().zip(&moved) {
            let (was, now) = (rects(before), rects(after));
            assert_eq!(was.len(), now.len());
            for (was, now) in was.iter().zip(&now) {
                assert!(
                    near(now.0, was.0 + dx) && near(now.1, was.1 + dy),
                    "page {}: the box at {was:?} is painted at {now:?}",
                    after.number,
                );
                assert!(near(now.2, was.2) && near(now.3, was.3));
                boxes += 1;
            }
            for (was, now) in content_items(before).iter().zip(content_items(after)) {
                if was.3.contains("lilliputian") {
                    assert!(near(now.0, was.0 + dx) && near(now.1, was.1 + dy));
                    quoted += 1;
                } else {
                    assert_eq!((was.0, was.1), (now.0, now.1), "{:?} moved", now.3);
                }
            }
        }
        assert!(boxes > 1, "the box is painted on {boxes} page(s)");
        assert!(quoted > 0);
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

    /// Acceptance: `h1::first-line` styles the text before a hard
    /// break in a heading, and the text after the break takes the
    /// heading's own style.
    #[test]
    fn a_first_line_rule_styles_a_heading_as_far_as_its_break() {
        let pages = paginate_styled(
            "h1 { font-size: 14pt } h1::first-line { font-size: 24pt }",
            vec![section(vec![
                broken_heading("Chapter One", "The Voyage to Lilliput"),
                paragraph("It began."),
            ])],
        );
        let run = |words: &str| {
            pages[0]
                .items
                .iter()
                .find_map(|item| match item {
                    DrawItem::Text { text, size, y, .. }
                        if text.to_lowercase().contains(&words.to_lowercase()) =>
                    {
                        Some((*size, *y))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{words:?} is not drawn"))
        };
        let (opening, top) = run("Chapter One");
        let (rest, under) = run("The Voyage");
        assert_eq!(opening, 24.0, "the text before the break");
        assert_eq!(rest, 14.0, "the text after the break");
        assert!(
            under > top,
            "the text after the break is not on a line under it"
        );
    }

    /// Acceptance: a heading with a hard break reads as one heading to
    /// `string-set`. The running head that prints it is one line, with
    /// a space where the break was.
    #[test]
    fn a_heading_with_a_break_sets_one_running_head() {
        let pages = paginate_styled(
            "h1 { string-set: chapter content() }
             @page :left { @top-left { content: string(chapter) } }
             @page :right { @top-right { content: string(chapter) } }",
            vec![section(vec![
                broken_heading("Chapter One", "The Voyage to Lilliput"),
                paragraph(&"my father had a small estate in nottinghamshire ".repeat(200)),
            ])],
        );
        assert!(pages.len() > 1, "the chapter fits on one page");
        let head: Vec<(String, f32)> = pages[1]
            .items
            .iter()
            .filter_map(|item| match item {
                DrawItem::Text { text, y, .. } => Some((text.to_lowercase(), *y)),
                _ => None,
            })
            .filter(|(text, _)| text.contains("chapter") || text.contains("voyage"))
            .collect();
        assert!(!head.is_empty(), "page 2 has no running head");
        assert!(
            head.iter().all(|(_, y)| *y == head[0].1),
            "the running head is not one line: {head:?}",
        );
        let printed: String = head.iter().map(|(text, _)| text.as_str()).collect();
        assert_eq!(printed, "chapter one the voyage to lilliput");
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

    /// Acceptance: an image warning names its url once, so the reader
    /// does not read the same name twice on one line.
    #[test]
    fn an_image_warning_names_its_url_once() {
        struct Tall;
        impl crate::images::ImageLoader for Tall {
            fn load(&self, url: &str) -> Option<Vec<u8>> {
                (url == "tall.png").then(|| png(768, 1536))
            }
        }

        let book = book_of(vec![section(vec![Block::Image {
            id: NodeId::UNASSIGNED,
            url: "tall.png".into(),
            alt: "a drawer of knives".into(),
            attributes: Attributes::default(),
            position: Some(SourcePos { line: 9, column: 1 }),
            span: None,
        }])]);
        let styles = crate::style::defaults(&book, registry());
        let assets = crate::images::Assets::probe(&book, &styles, &Tall);
        let output = layout_book(&book, &styles, registry(), &assets);

        let warning = output
            .warnings
            .iter()
            .find(|warning| warning.message.contains("tall.png"))
            .expect("scaling an image to fit is worth saying");
        assert_eq!(warning.message.matches("tall.png").count(), 1);
        assert!(warning.message.ends_with('.'), "{}", warning.message);
        assert!(!warning.message.contains(';'), "{}", warning.message);
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
        let assets = crate::images::Assets::probe(&book, &styles, &Png);
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
        assert!(warning.message.contains("is taller than the page"));
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

    /// An `h4` with the words `value`.
    fn h4(value: &str) -> Block {
        let Block::Heading { inlines, .. } = heading(value) else {
            unreachable!("`heading` makes a heading");
        };
        Block::Heading {
            id: NodeId::UNASSIGNED,
            level: HeadingLevel::H4,
            inlines,
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    /// Each content line of a page as the text of its runs, with its
    /// baseline, its leading edge and its size.
    fn read_lines(page: &Page) -> Vec<(f32, f32, f32, String)> {
        content_lines(page)
            .into_iter()
            .map(|(baseline, runs)| {
                let text = runs.iter().map(|run| run.2).collect();
                (baseline, runs[0].0, runs[0].1, text)
            })
            .collect()
    }

    /// The width of a line of text on the opening page of a chapter.
    fn opening_measure() -> f32 {
        master(Situation::First(Side::Recto)).geometry.measure()
    }

    /// Part: a generated box holds the value of `content`, which is a
    /// string or nothing.
    #[test]
    fn a_generated_box_holds_its_string_or_nothing() {
        let blocks = || vec![section(vec![paragraph("the paragraph itself")])];
        let said = paginate_styled("p::before { content: \"Before it\" }", blocks());
        let texts: Vec<String> = read_lines(&said[0])
            .into_iter()
            .map(|line| line.3)
            .collect();
        assert_eq!(texts, ["Before it", "the paragraph itself"]);

        let bare = paginate(blocks());
        let empty = paginate_styled("p::before { content: \"\" }", blocks());
        assert_eq!(read_lines(&empty[0]), read_lines(&bare[0]));
    }

    /// Part: the generated box is a block in the flow, the first or
    /// the last child of its element, across the element's content box
    /// and in the element's style.
    #[test]
    fn a_generated_box_is_the_first_or_last_child_of_its_block() {
        let pages = paginate_styled(
            "blockquote { font-size: 10pt; margin: 0 20pt } \
             blockquote::before { content: \"first\" } \
             blockquote::after { content: \"last\" }",
            vec![section(vec![
                quote(vec![paragraph("inside one"), paragraph("inside two")]),
                paragraph("outside"),
            ])],
        );
        let lines = read_lines(&pages[0]);
        let texts: Vec<&str> = lines.iter().map(|line| line.3.as_str()).collect();
        assert_eq!(
            texts,
            ["first", "inside one", "inside two", "last", "outside"]
        );
        let (left, _) = origin_of(&pages[0]);
        for (_, x, size, text) in [&lines[0], &lines[3]] {
            assert!((x - (left + 20.0)).abs() < 1e-3, "{text} starts at {x}");
            assert_eq!(*size, 10.0, "{text} is not in the quotation's size");
        }
    }

    /// Part: the generated box takes the box model. Its border box
    /// holds its border, its padding and its height, inside its own
    /// margins.
    #[test]
    fn a_generated_box_takes_the_box_model() {
        const TINT: Color = Color::rgb(0xee, 0xee, 0xee);
        const INK: Color = Color::rgb(0xd6, 0x07, 0x5e);
        let blocks = || vec![section(vec![heading("Title"), paragraph("after")])];
        let pages = paginate_styled(
            "h1::after { content: \"\"; height: 2pt; padding: 3pt 0; \
             border-top: 1pt solid #d6075e; margin: 0 12pt; background-color: #eeeeee }",
            blocks(),
        );
        let (left, _) = origin_of(&pages[0]);
        let measure = opening_measure();
        let boxes = rects(&pages[0]);
        let [(x, _, w, h, _)] = boxes
            .iter()
            .filter(|rect| rect.4 == TINT)
            .collect::<Vec<_>>()[..]
        else {
            panic!("one background: {boxes:?}");
        };
        assert!((x - (left + 12.0)).abs() < 1e-3, "the box starts at {x}");
        assert!((w - (measure - 24.0)).abs() < 1e-3, "the box is {w} wide");
        assert!((h - 9.0).abs() < 1e-3, "the box is {h} tall");
        assert!(
            boxes
                .iter()
                .any(|rect| rect.4 == INK && (rect.3 - 1.0).abs() < 1e-3),
            "no top border: {boxes:?}",
        );
        let moved = baseline_of(&pages, "after") - baseline_of(&paginate(blocks()), "after");
        assert!((moved - 9.0).abs() < 1e-3, "the prose moved {moved}");
    }

    /// Acceptance: `h4::after { content: ""; height: 2pt;
    /// background-color: #d6075e }` draws a rule under every h4, at
    /// the width of the heading's content box.
    #[test]
    fn a_generated_rule_sits_under_every_h4_across_its_content_box() {
        const INK: Color = Color::rgb(0xd6, 0x07, 0x5e);
        let pages = paginate_styled(
            "h4 { margin: 0 20pt; padding: 0 10pt } \
             h4::after { content: \"\"; height: 2pt; background-color: #d6075e }",
            vec![section(vec![
                h4("First"),
                paragraph("one"),
                h4("Second"),
                paragraph("two"),
            ])],
        );
        let page = &pages[0];
        let (left, _) = origin_of(page);
        let measure = opening_measure();
        let lines = read_lines(page);
        let rules: Vec<_> = rects(page)
            .into_iter()
            .filter(|rect| rect.4 == INK)
            .collect();
        assert_eq!(rules.len(), 2, "{rules:?}");
        for ((x, y, w, h, _), title) in rules.iter().zip(["First", "Second"]) {
            assert!(
                (x - (left + 30.0)).abs() < 1e-3,
                "{title}: the rule starts at {x}"
            );
            assert!(
                (w - (measure - 60.0)).abs() < 1e-3,
                "{title}: the rule is {w} wide"
            );
            assert!((h - 2.0).abs() < 1e-3, "{title}: the rule is {h} tall");
            let at = lines
                .iter()
                .position(|line| line.3 == title)
                .expect("the heading is set");
            assert!(*y > lines[at].0, "{title}: the rule is above the baseline");
            assert!(
                y + h <= lines[at + 1].0,
                "{title}: the rule runs into the prose"
            );
        }
    }

    /// Acceptance: `blockquote::before { content: "\201C" }` sets an
    /// opening quotation mark before the quote's first line.
    #[test]
    fn a_generated_quotation_mark_opens_the_quotation() {
        let pages = paginate_styled(
            "blockquote::before { content: \"\\201C\" }",
            vec![section(vec![quote(vec![paragraph("quoted words")])])],
        );
        let lines = read_lines(&pages[0]);
        let texts: Vec<&str> = lines.iter().map(|line| line.3.as_str()).collect();
        assert_eq!(texts, ["\u{201C}", "quoted words"]);
        let (left, _) = origin_of(&pages[0]);
        assert!(
            (lines[0].1 - (left + quote_indent())).abs() < 1e-3,
            "the mark starts at {}",
            lines[0].1,
        );
    }

    /// Acceptance: a generated box takes height in the flow, and the
    /// block under it moves down by that height.
    #[test]
    fn the_block_under_a_generated_box_moves_down_by_its_height() {
        let blocks = || vec![section(vec![paragraph("one"), paragraph("two")])];
        let bare = paginate(blocks());
        let tall = paginate_styled("p::after { content: \"\"; height: 30pt }", blocks());
        let moved = baseline_of(&tall, "two") - baseline_of(&bare, "two");
        assert!((moved - 30.0).abs() < 1e-3, "the paragraph moved {moved}");
    }

    /// Acceptance: a page break falling on a generated box keeps it
    /// with the element it belongs to. No page opens between a box and
    /// its element, so no page opens on a box that `::after` made.
    #[test]
    fn a_page_break_keeps_a_generated_box_with_its_element() {
        let book = book_of(vec![section(vec![paragraph(&"words ".repeat(60))])]);
        let fragments = |css: &str| {
            let styles = styled(css, &book);
            Paginator::new(registry(), &styles).section_fragments(&book.sections[0])
        };
        let lines = |fragments: &[Fragment]| -> Vec<usize> {
            fragments
                .iter()
                .enumerate()
                .filter(|(_, fragment)| matches!(fragment.piece, Piece::Line { .. }))
                .map(|(index, _)| index)
                .collect()
        };
        let bare = fragments("");
        assert_ne!(bare[lines(&bare)[0]].break_before, BreakPoint::Forbidden);

        let boxed =
            fragments("p::before { content: \"x\" } p::after { content: \"\"; height: 20pt }");
        let at = lines(&boxed);
        assert_eq!(
            boxed[at[1]].break_before,
            BreakPoint::Forbidden,
            "the paragraph can leave the box before it",
        );
        let after = at.last().expect("the paragraph has lines") + 1;
        assert!(
            after < boxed.len(),
            "the box after the paragraph is missing"
        );
        for fragment in &boxed[after..] {
            assert_eq!(
                fragment.break_before,
                BreakPoint::Forbidden,
                "the box after the paragraph can leave it",
            );
        }

        let pages = paginate_styled(
            "p { orphans: 1; widows: 1 } \
             p::after { content: \"\"; height: 24pt; background-color: #eeeeee }",
            vec![section(long_prose(40))],
        );
        assert!(pages.len() > 2, "the prose fills {} pages", pages.len());
        for page in &pages {
            let lines = content_lines(page);
            for (_, y, ..) in rects(page) {
                assert!(
                    lines.first().is_some_and(|(baseline, _)| *baseline < y),
                    "page {}: a generated box opens the page",
                    page.number,
                );
            }
        }
    }
}
