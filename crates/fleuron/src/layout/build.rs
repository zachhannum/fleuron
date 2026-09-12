//! Blocks in, fragments out: margins folded together, breaks
//! settled, lines broken, the initial letter set beside them.

use std::borrow::Cow;
use std::sync::Arc;

use crate::content::{
    Attributes, Block, Inline, NodeId, Section, block_attributes, block_id, inline_attributes,
    origin, text,
};
use crate::lines::{Line, LineBreakOptions, Measure, Opening, Patterns, Shaped, Span};
use crate::style::{
    Break, ColumnSpan, ComputedStyle, Content, Hyphens, Position, StringPiece, StyleTree,
    TextAlign, TextJustify,
};

use super::Paginator;
use super::cap::Cap;
use super::fragment::{
    BreakPoint, Decoration, Decorations, DropCap, Fragment, Marks, Piece, decorated, decoration,
};
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
        let start = builder.open(&style, &[], 0.0, measure);
        let column = style.content_box(0.0, measure);
        let whole = style.content_box(0.0, geometry.content_size().0);
        for block in &section.blocks {
            builder.spanning = self.styles.style(block_id(block)).column_span == ColumnSpan::All;
            let (x, measure) = if builder.spanning { whole } else { column };
            let first = builder.fragments.len();
            builder.blocks(std::slice::from_ref(block), x, measure);
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
    /// The decorated blocks still open, outermost first.
    open: Vec<Pending>,
    /// Whether the block being built spans every column.
    spanning: bool,
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
        }
    }
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
        style: &ComputedStyle,
        inlines: &[Inline],
        x: f32,
        measure: f32,
    ) -> usize {
        self.ask(style.break_before);
        self.mark(style, inlines);
        self.margin = self.margin.max(style.margin.top);
        let start = self.fragments.len();
        let border = style.border.widths();
        let backdrop = self.paginator.backdrop(&style.background);
        if let Some(decoration) = decoration(style, x, measure, backdrop) {
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

    /// Records the id an element carries. It lands on the first
    /// fragment the element emits, the way a string it sets does, so
    /// the page that fragment lands on is the page a reference to the
    /// element prints.
    fn name(&mut self, attributes: &Attributes) {
        if let Some(id) = &attributes.id {
            let marks = self.pending_marks.get_or_insert_with(Box::default);
            marks.targets.push(id.clone());
        }
    }

    /// The same for the inlines of one block, which land on the
    /// block's first line.
    fn name_inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            self.name(inline_attributes(inline));
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

    /// Every block of one nesting level, at `x` from the content
    /// box's leading edge and breaking to `measure`.
    pub(super) fn blocks(&mut self, blocks: &[Block], x: f32, measure: f32) {
        for block in blocks {
            self.name(block_attributes(block));
            match block {
                Block::Heading { id, inlines, .. } | Block::Paragraph { id, inlines, .. } => {
                    self.name_inlines(inlines);
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
                    // An image against the page is not in the flow:
                    // it takes no space here, and the margins that
                    // met around it still meet.
                    if style.position == Position::Absolute {
                        self.anchor(*id);
                        continue;
                    }
                    let start = self.open(&style, &[], x, measure);
                    self.image(&style, url, origin(self.source, *position), x, measure);
                    self.close(&style, start);
                }
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
            self.paginator.missing(url, origin);
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
                format!("Image {url} is taller than the page. It is scaled to fit."),
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
pub(super) fn carry_over(fresh: &mut [Fragment], old: &[Fragment]) {
    let (Some(head), Some(last)) = (old.first(), old.last()) else {
        return;
    };
    for fragment in fresh.iter_mut() {
        fragment.spanning = head.spanning;
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
        first.fixed += head.fixed;
        first.marks = head.marks.clone();
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
    use crate::content::{Attributes, Block, NodeId, Section, SourcePos};
    use crate::layout::testing::{
        assert_orphans_and_widows, book_of, chapter_size, content_lines, folio_size, heading,
        long_prose, master, origin_of, ornament, paginate, paginate_styled, paragraph, png, prose,
        quote, registry, right_edge, scene_break, section, small_caps_lines, styled, tagged_prose,
        ua, under_h3,
    };
    use crate::layout::{BreakPoint, Fragment, Paginator, Piece, layout_book};
    use crate::pages::{DrawItem, Side};
    use crate::style::Situation;

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
}
