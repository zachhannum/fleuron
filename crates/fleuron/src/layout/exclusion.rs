//! The images the sheet lifts out of the flow, and the bands the
//! prose around one is set in.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::content::{Block, Book, NodeId, PseudoElement, block_position, cell_blocks, origin};
use crate::lines::{Measure, Span};
use crate::pages::{DrawItem, PageBox};
use crate::style::{ComputedStyle, Edges, Inset, PageGeometry, Position, ShapeOutside, WrapFlow};

use super::Paginator;
use super::build::{Builder, Child, Reflow, carry_over, children, set_lines};
use super::cap::Cap;
use super::flow::{Checkpoint, Flow, Placed, shift, shift_boxes};
use super::fragment::Fragment;

impl Paginator<'_> {
    /// The page each of `all` lands on, found by a flow that paints
    /// nothing. `fragments` gives the fragments of the section at an
    /// index of `book.sections`.
    ///
    /// The flow puts a box on a page when a page closes with the
    /// anchor of that box on it. A page that a box changes is laid out
    /// again, from the item that was being placed when that page
    /// opened. A box that pushes its own anchor off its page goes on
    /// the next page instead, and moves no more.
    ///
    /// A section stays in hand while a page that opened in it can
    /// still be laid out again.
    pub(super) fn settle<'f>(
        &self,
        book: &Book,
        all: Vec<Anchored>,
        mut fragments: impl FnMut(usize) -> Cow<'f, [Fragment]>,
    ) -> AnchoredBoxes {
        let mut flow = Flow::settling(
            self,
            AnchoredBoxes {
                all,
                by_page: BTreeMap::new(),
            },
        );
        let mut held: BTreeMap<usize, Cow<'f, [Fragment]>> = BTreeMap::new();
        let (mut section, mut item, mut opened) = (0, 0, false);
        loop {
            if let Some((at, from)) = flow.again() {
                (section, item, opened) = (at, from, true);
            }
            let Some(content) = book.sections.get(section) else {
                flow.close_book();
                if flow.settling.as_ref().is_some_and(|s| s.redo.is_some()) {
                    continue;
                }
                break;
            };
            let oldest = flow.oldest_section().unwrap_or(section);
            held.retain(|index, _| *index >= oldest);
            let built = held.entry(section).or_insert_with(|| fragments(section));
            if !opened {
                flow.open_section(content);
                opened = true;
            }
            if item >= built.len() {
                (section, item, opened) = (section + 1, 0, false);
                continue;
            }
            flow.mark(section, item);
            item = flow.item(built, item);
        }
        flow.anchored
    }

    /// The images and blocks the sheet anchored to the page, in
    /// document order.
    ///
    /// A box inside a block that is anchored as well lands on the page
    /// that block lands on.
    pub(super) fn anchored_boxes(&self, book: &Book) -> Vec<Anchored> {
        fn walk<'b>(
            paginator: &Paginator,
            boxes: impl IntoIterator<Item = Child<'b>>,
            source: Option<&str>,
            around: Around,
            anchored: &mut Vec<Anchored>,
        ) {
            let styles = paginator.styles;
            for child in boxes {
                let id = child.id();
                let style = styles.style(id);
                let lifted = style.position == Position::Absolute;
                let opacity = around.opacity * style.opacity;
                let node = if lifted {
                    let node = around.node.unwrap_or(id);
                    anchored.extend(match child {
                        Child::Block(Block::Image { url, position, .. }) => paginator
                            .anchored_image(
                                node,
                                id,
                                style,
                                url,
                                origin(source, *position),
                                opacity,
                            ),
                        _ => Some(paginator.anchored_block(
                            node,
                            child,
                            style,
                            source,
                            around.opacity,
                        )),
                    });
                    Some(node)
                } else {
                    around.node
                };
                let host = Around { node, opacity };
                let Child::Block(block) = child else {
                    continue;
                };
                let position = block_position(block);
                let pseudo = |which| {
                    styles
                        .pseudo_element(id, which)
                        .map(|id| Child::Generated(id, position))
                };
                match block {
                    Block::Blockquote { blocks, .. } => walk(
                        paginator,
                        children(styles, id, blocks, position),
                        source,
                        host,
                        anchored,
                    ),
                    Block::List { items, .. } => {
                        walk(
                            paginator,
                            pseudo(PseudoElement::Before),
                            source,
                            host,
                            anchored,
                        );
                        for item in items {
                            walk(
                                paginator,
                                children(styles, item.id, &item.blocks, item.position),
                                source,
                                host,
                                anchored,
                            );
                        }
                        walk(
                            paginator,
                            pseudo(PseudoElement::After),
                            source,
                            host,
                            anchored,
                        );
                    }
                    Block::Table { head, body, .. } => {
                        walk(
                            paginator,
                            pseudo(PseudoElement::Before),
                            source,
                            host,
                            anchored,
                        );
                        for blocks in cell_blocks(head, body) {
                            walk(
                                paginator,
                                blocks.iter().map(Child::Block),
                                source,
                                host,
                                anchored,
                            );
                        }
                        walk(
                            paginator,
                            pseudo(PseudoElement::After),
                            source,
                            host,
                            anchored,
                        );
                    }
                    // An image against the page is placed whole, with no
                    // children in it.
                    Block::Image { .. } if lifted => {}
                    _ => walk(
                        paginator,
                        children(styles, id, &[], position),
                        source,
                        host,
                        anchored,
                    ),
                }
            }
        }
        let mut anchored = Vec::new();
        for section in &book.sections {
            walk(
                self,
                children(self.styles, section.id, &section.blocks, section.position),
                section.source.as_deref(),
                Around {
                    node: None,
                    opacity: self.styles.style(section.id).opacity,
                },
                &mut anchored,
            );
        }
        anchored
    }

    /// One image anchored to the page, which lands on the page `node`
    /// lands on.
    ///
    /// It is sized as a block image is, with a percentage measuring
    /// the page area, and scaled down where that does not fit it.
    /// `opacity` is its own and that of the blocks around it,
    /// multiplied together.
    fn anchored_image(
        &self,
        node: NodeId,
        id: NodeId,
        style: &ComputedStyle,
        url: &str,
        origin: String,
        opacity: f32,
    ) -> Option<Anchored> {
        let Some((asset, intrinsic)) = self.assets.lookup(url) else {
            self.missing(url, origin);
            return None;
        };
        let (available, room) = self.styles.default_page().geometry.content_size();
        let margin = style.margin;
        let super::image::ImageSize { width, height, .. } = self.image_size(
            style,
            url,
            intrinsic.size(),
            (available, room),
            (available - margin.inline()).max(0.0),
            (room - margin.top - margin.bottom).max(0.0),
            (!origin.is_empty()).then_some(origin),
        );
        Some(Anchored {
            node,
            width: width + margin.inline(),
            height: height + margin.top + margin.bottom,
            inset: style.inset,
            wrap: style.wrap_flow,
            shape: self.shape(style, Some(asset), (width, height)),
            layer: style.z_index,
            opacity,
            paint: Paint::Image {
                id,
                asset,
                x: margin.left,
                y: margin.top,
                width,
                height,
            },
        })
    }

    /// One block anchored to the page, which lands on the page `node`
    /// lands on, laid out on its own.
    ///
    /// Its lines break to the width that its insets leave of the page
    /// area. With both insets set, that is the width between them. With
    /// one set, it runs from that inset to the far edge. With neither
    /// set, it is `geometry.measure()`.
    ///
    /// `around` is the `opacity` of the blocks around it, multiplied
    /// together. The block multiplies its own into that.
    fn anchored_block(
        &self,
        node: NodeId,
        child: Child<'_>,
        style: &ComputedStyle,
        source: Option<&str>,
        around: f32,
    ) -> Anchored {
        let geometry = self.styles.default_page().geometry;
        let (area, _) = geometry.content_size();
        let (left, right) = (
            style.inset.left.resolve(area),
            style.inset.right.resolve(area),
        );
        let width = match (left, right) {
            (None, None) => geometry.measure(),
            _ => (area - left.unwrap_or(0.0) - right.unwrap_or(0.0)).max(0.0),
        };
        let mut builder = Builder::new(self, source);
        builder.lifted = Some(child.id());
        builder.opacity = around;
        builder.blocks([child], 0.0, width);
        let stacked = builder.stack();
        let margin = style.margin;
        let inner = (
            (width - margin.inline()).max(0.0),
            (stacked.height - margin.top - margin.bottom).max(0.0),
        );
        Anchored {
            node,
            width,
            height: stacked.height,
            inset: style.inset,
            wrap: style.wrap_flow,
            shape: self.shape(style, None, inner),
            layer: style.z_index,
            opacity: style.opacity * around,
            paint: Paint::Block(stacked.items, stacked.boxes),
        }
    }

    /// The contour one anchored box's prose keeps clear of, in the
    /// coordinates of the box its insets place. `size` is the box
    /// inside its margins.
    ///
    /// `auto` is what the trace stage left, laid over the image
    /// inside its margins. A block, and an image with no alpha from the
    /// tracer, give their box. The percentages of a polygon are
    /// percentages of the whole box, margins included.
    fn shape(&self, style: &ComputedStyle, asset: Option<u32>, size: (f32, f32)) -> Option<Shape> {
        // The one predicate the trace stage keys on as well, so that
        // what is traced and what is read cannot drift apart.
        if !style.excludes() {
            return None;
        }
        let margin = style.margin;
        let (width, height) = size;
        let rings = match &style.shape_outside {
            ShapeOutside::None => return None,
            ShapeOutside::Auto => self
                .contours
                .get(asset?)?
                .rings
                .iter()
                .map(|ring| {
                    ring.iter()
                        .map(|[x, y]| [margin.left + x * width, margin.top + y * height])
                        .collect()
                })
                .collect(),
            ShapeOutside::Polygon(points) => {
                let box_ = (width + margin.inline(), height + margin.top + margin.bottom);
                vec![
                    points
                        .iter()
                        .map(|point| [point.x.to_points(box_.0), point.y.to_points(box_.1)])
                        .collect(),
                ]
            }
        };
        Some(Shape {
            rings,
            margin: style.shape_margin,
        })
    }
}

/// One image or block the sheet lifted out of the flow: what it
/// paints, how far its insets put it from the page area, and which
/// side the prose wraps on.
#[derive(Debug, Clone)]
pub(super) struct Anchored {
    /// The node whose place in the flow decides its page: its own, or
    /// the anchored block it sits inside.
    node: NodeId,
    /// Width in points, margins included.
    width: f32,
    /// Height in points, margins included.
    height: f32,
    /// What the insets say about where it sits.
    inset: Edges<Inset>,
    /// Which side of it the prose wraps on.
    wrap: WrapFlow,
    /// The contour the prose keeps clear of in place of the box,
    /// from `shape-outside`.
    shape: Option<Shape>,
    /// The layer an image paints in, from its own `z-index`. The
    /// items of a block carry the layers of the blocks they came out
    /// of.
    layer: i32,
    /// How much of an image shows, from 0 to 1: its own `opacity` and
    /// that of the blocks around it. The items of a block already
    /// show as much as the blocks they came out of let them.
    opacity: f32,
    /// What it paints, from the top left corner of its margin box.
    paint: Paint,
}

/// What the blocks around one box in the walk come to: the anchored
/// block it sits inside, and their `opacity` multiplied together.
#[derive(Debug, Clone, Copy)]
struct Around {
    node: Option<NodeId>,
    opacity: f32,
}

/// What one anchored box paints.
#[derive(Debug, Clone)]
enum Paint {
    /// An image, inside the margins it keeps.
    Image {
        /// The image's own node.
        id: NodeId,
        /// Index into the asset table.
        asset: u32,
        /// Leading edge, from the margin box's own.
        x: f32,
        /// Top, from the margin box's own.
        y: f32,
        /// Width in points, after any scaling.
        width: f32,
        /// Height in points, after any scaling.
        height: f32,
    },
    /// What a block was laid out as, and the border boxes of the blocks
    /// in it, from the top left corner of its margin box.
    Block(Vec<DrawItem>, Vec<(NodeId, PageBox)>),
}

/// A contour in the coordinates of the box the insets place: `(0, 0)`
/// its top left corner, points down and across from there.
#[derive(Debug, Clone)]
struct Shape {
    /// The rings, each closed by its own first point.
    rings: Vec<Vec<[f32; 2]>>,
    /// How far the prose keeps off them, from `shape-margin`.
    margin: f32,
}

impl Anchored {
    /// What it keeps to itself on a page of this geometry: its margin
    /// box.
    ///
    /// An inset is a distance from the page area, the box inside the
    /// margins. A negative inset reaches into the margin. A percentage
    /// is a percentage of the width or the height of the page area.
    /// Where both insets of an axis are set, the leading inset places
    /// the box. Where neither is set, the box sits at the edge of the
    /// page area.
    fn rect(&self, geometry: PageGeometry) -> Rect {
        let (left, top) = geometry.content_origin();
        let (width, height) = geometry.content_size();
        let (w, h) = (self.width, self.height);
        let place =
            |start: Option<f32>, end: Option<f32>, origin: f32, available: f32, size: f32| match (
                start, end,
            ) {
                (Some(start), _) => origin + start,
                (None, Some(end)) => origin + available - end - size,
                (None, None) => origin,
            };
        Rect {
            x: place(
                self.inset.left.resolve(width),
                self.inset.right.resolve(width),
                left,
                width,
                w,
            ),
            y: place(
                self.inset.top.resolve(height),
                self.inset.bottom.resolve(height),
                top,
                height,
                h,
            ),
            w,
            h,
        }
    }

    /// What it paints on a page of this geometry.
    pub(super) fn items(&self, geometry: PageGeometry) -> Vec<DrawItem> {
        let rect = self.rect(geometry);
        match &self.paint {
            Paint::Image {
                asset,
                x,
                y,
                width,
                height,
                ..
            } => vec![DrawItem::Image {
                x: rect.x + x,
                y: rect.y + y,
                w: *width,
                h: *height,
                asset: *asset,
                alpha: crate::pages::fade(255, self.opacity),
                layer: self.layer,
            }],
            Paint::Block(items, _) => {
                let mut items = items.clone();
                shift(&mut items, rect.x, rect.y);
                items
            }
        }
    }

    /// The border boxes it takes on a page of this geometry: an
    /// image's own, or those of the blocks a block is made of.
    pub(super) fn boxes(&self, geometry: PageGeometry) -> Vec<(NodeId, PageBox)> {
        let rect = self.rect(geometry);
        match &self.paint {
            Paint::Image {
                id,
                x,
                y,
                width,
                height,
                ..
            } => vec![(
                *id,
                PageBox {
                    page: 0,
                    x: rect.x + x,
                    y: rect.y + y,
                    width: *width,
                    height: *height,
                },
            )],
            Paint::Block(_, boxes) => {
                let mut boxes = boxes.clone();
                shift_boxes(&mut boxes, rect.x, rect.y);
                boxes
            }
        }
    }
}

/// The images and blocks one book anchors, and the page each one
/// landed on.
///
/// The flow gives the page that a box falls on. The sheet gives the
/// place of the box on that page. [`Paginator::settle`] finds the
/// pages, and the flow that paints keeps them.
#[derive(Debug, Default)]
pub(crate) struct AnchoredBoxes {
    pub(super) all: Vec<Anchored>,
    /// Which boxes a page carries, by page index, each list in
    /// document order.
    pub(super) by_page: BTreeMap<usize, Vec<usize>>,
}

impl AnchoredBoxes {
    /// Whether the book anchors nothing.
    pub(super) fn is_empty(&self) -> bool {
        self.all.is_empty()
    }

    /// Whether the prose of the page at `index` wraps around a box.
    pub(super) fn wraps(&self, index: usize) -> bool {
        self.by_page
            .get(&index)
            .is_some_and(|boxes| boxes.iter().any(|at| self.all[*at].wrap != WrapFlow::Auto))
    }
}

/// Where one box stands while the flow finds its page.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Landing {
    /// No page closed with its anchor on it yet.
    Waiting,
    /// On the page that closed with its anchor on it.
    On(usize),
    /// It pushed its own anchor off its page. It goes on the first
    /// page from `from` that closes once its anchor has passed.
    Pushed { from: usize, passed: bool },
    /// On a page it does not leave.
    Settled(usize),
}

/// What the flow that finds the page of each box keeps to lay a page
/// out again.
pub(super) struct Settling {
    /// Where each box stands, by its index in `AnchoredBoxes::all`.
    landings: Vec<Landing>,
    /// The flow before the item being placed.
    before: Option<Rc<Checkpoint>>,
    /// The flow before the item that opened the page being built.
    page: Option<Rc<Checkpoint>>,
    /// Where to start again, once the item being placed is done.
    pub(super) redo: Option<Rc<Checkpoint>>,
}

impl Settling {
    pub(super) fn new(anchored: &AnchoredBoxes) -> Settling {
        Settling {
            landings: vec![Landing::Waiting; anchored.all.len()],
            before: None,
            page: None,
            redo: None,
        }
    }

    /// Records that a page opened during the item being placed.
    pub(super) fn opened(&mut self) {
        self.page.clone_from(&self.before);
    }
}

impl Flow<'_, '_> {
    /// Keeps the flow as it stands before the item at `item` of the
    /// section at `section` is placed.
    pub(super) fn mark(&mut self, section: usize, item: usize) {
        if self.settling.is_none() {
            return;
        }
        let checkpoint = Rc::new(self.checkpoint(section, item));
        let Some(settling) = self.settling.as_mut() else {
            return;
        };
        settling.page.get_or_insert_with(|| checkpoint.clone());
        settling.before = Some(checkpoint);
    }

    /// The section that a page can be laid out again from.
    pub(super) fn oldest_section(&self) -> Option<usize> {
        let settling = self.settling.as_ref()?;
        settling.page.as_ref().map(|page| page.section)
    }

    /// Puts the flow back where a page that a box changed opened, and
    /// answers the item to place from there.
    pub(super) fn again(&mut self) -> Option<(usize, usize)> {
        let checkpoint = self.settling.as_mut()?.redo.take()?;
        self.restore(&checkpoint);
        let settling = self.settling.as_mut()?;
        settling.page = Some(checkpoint.clone());
        settling.before = Some(checkpoint.clone());
        Some((checkpoint.section, checkpoint.item))
    }

    /// Moves boxes onto the page that is closing with `placed` on it,
    /// and off it, and asks for the page again where that changes a
    /// line.
    ///
    /// A box goes on the page that closes with its anchor on it. A box
    /// on this page whose anchor is not on it pushed that anchor to a
    /// later page, so it comes off. Once a page is asked for again,
    /// nothing moves until the flow is back at that page.
    pub(super) fn land(&mut self, placed: &[Placed]) {
        let index = self.pages.len();
        let Some(settling) = self.settling.as_mut() else {
            return;
        };
        if settling.redo.is_some() {
            return;
        }
        let bound: BTreeSet<NodeId> = placed
            .iter()
            .flat_map(|placed| placed.anchors.iter().copied())
            .collect();
        let mut again = false;
        for (at, anchored) in self.anchored.all.iter().enumerate() {
            let here = bound.contains(&anchored.node);
            let landing = match settling.landings[at] {
                Landing::Waiting if here => Landing::On(index),
                Landing::On(page) if page == index && !here => Landing::Pushed {
                    from: index + 1,
                    passed: false,
                },
                Landing::Pushed { from, passed } if index >= from && (passed || here) => {
                    Landing::Settled(index)
                }
                Landing::Pushed { from, .. } if here => Landing::Pushed { from, passed: true },
                landing => landing,
            };
            let before = std::mem::replace(&mut settling.landings[at], landing);
            let boxes = self.anchored.by_page.entry(index).or_default();
            match (before, landing) {
                (Landing::On(_), Landing::Pushed { .. }) => boxes.retain(|other| *other != at),
                (_, Landing::On(_) | Landing::Settled(_)) if before != landing => {
                    let slot = boxes.partition_point(|other| *other < at);
                    boxes.insert(slot, at);
                }
                _ => continue,
            }
            again |= anchored.wrap != WrapFlow::Auto;
        }
        self.anchored.by_page.retain(|_, boxes| !boxes.is_empty());
        if again {
            settling.redo.clone_from(&settling.page);
        }
    }

    /// Places one paragraph, and breaks it again where an image
    /// narrows the bands it is set in.
    ///
    /// This is the one thing the flow measures. Everywhere else a
    /// fragment arrives with its box decided. Here the box depends on
    /// where the paragraph lands. The flow breaks the paragraph again
    /// through `LineLayout`, and what comes back is what it places.
    ///
    /// A page boundary inside the paragraph starts it over. The flow
    /// takes the lines that crossed off the fresh column and sets the
    /// rest of the paragraph again from where they now sit. The page
    /// index only rises, so the flow breaks the paragraph at most
    /// twice on any page it tries.
    pub(super) fn paragraph(&mut self, original: &[Fragment], reflow: &Reflow) {
        let mut set: Cow<'_, [Fragment]> = Cow::Borrowed(original);
        let mut ends: Cow<'_, [usize]> = Cow::Borrowed(&reflow.ends);
        let mut at = 0;
        // Whether what is in hand was broken beside an image. The
        // lines a section arrives with fit any page. Lines broken
        // against a notch fit the page they were broken on, so a page
        // boundary under them is a break to do again.
        let mut narrowed = false;
        while at < set.len() {
            if set[at].spanning != self.tier().spanning {
                self.open_tier(set[at].spanning);
            }
            // The profile is read against where the line sits, which
            // is where `place` is about to put it.
            let lead = if self.opening() { 0.0 } else { set[at].lead };
            let from = if at == 0 { 0 } else { ends[at - 1] };
            // A line set again keeps none of the gap it was broken
            // with. That gap was the page it left.
            let fixed = if from == 0 { original[0].fixed } else { 0.0 };
            let top = self.cursor + lead + fixed;
            // The markers of the items the paragraph opens hang before
            // its first line, and keep clear of an image as it does.
            let hung = original[0]
                .markers
                .as_ref()
                .filter(|_| from == 0)
                .map(|markers| {
                    markers
                        .iter()
                        .map(|marker| marker.x)
                        .fold(f32::MAX, f32::min)
                        - reflow.setting.x
                });
            let profile = self.profile(top, reflow, from == 0, hung);
            if profile.is_some() || narrowed {
                narrowed = profile.is_some();
                let profile = profile.unwrap_or_else(|| Profile::plain(reflow, from == 0));
                let paginator = self.paginator;
                if self.paints {
                    paginator.rebreaks.set(paginator.rebreaks.get() + 1);
                }
                let broken = paginator
                    .lines
                    .rebreak(&reflow.shaped, &profile.measure, from);
                if broken.lines.is_empty() {
                    return;
                }
                let mut fresh = set_lines(
                    self.paginator,
                    broken.lines,
                    &profile.measure,
                    &profile.gaps,
                    &reflow.setting.wrapped(from == 0, profile.letter),
                );
                carry_over(&mut fresh, &set[at..], fixed);
                if from == 0
                    && let Some(first) = fresh.first_mut()
                {
                    let moved = profile.measure.at(0).origin - reflow.base.at(0).origin;
                    first.markers = original[0].markers.clone().map(|mut markers| {
                        for marker in markers.iter_mut() {
                            marker.x += moved;
                        }
                        markers
                    });
                }
                set = Cow::Owned(fresh);
                ends = Cow::Owned(broken.ends);
                at = 0;
            }
            let mut split = None;
            for index in at..set.len() {
                let opened = (self.pages.len(), self.column);
                self.place(&set[index]);
                if (self.pages.len(), self.column) != opened {
                    split = Some(index);
                    break;
                }
            }
            let Some(index) = split else { return };
            // What crossed the boundary is this paragraph's again:
            // the fresh column is where the rest of it starts.
            let crossed = (index + 1 - at).min(self.placed.len() - self.column_start);
            at = index + 1 - crossed;
            self.unplace(crossed);
        }
    }

    /// Takes the last `count` fragments off the column being filled,
    /// so the paragraph they came from can be set again where they
    /// now sit.
    fn unplace(&mut self, count: usize) {
        let keep = self.placed.len() - count;
        for placed in self.placed.drain(keep..) {
            self.pending_anchors.extend(placed.anchors);
        }
        self.cursor = self.placed[self.column_start..]
            .last()
            .map(|placed| placed.top + placed.height)
            .unwrap_or(self.tier().top);
    }

    /// The images on the page being built that reach the column being
    /// filled, in that column's coordinates.
    ///
    /// An image is placed against the page and a column is a region
    /// of the page, so which images a column sets around is the
    /// overlap of the two. An image that reaches no part of this
    /// column is not one of its holes, and an image that reaches two
    /// columns is a hole in each of them. A paragraph that spans the
    /// columns reads the whole content box.
    fn holes(&self) -> Vec<Hole<'_>> {
        let index = self.pages.len();
        let Some(anchored) = self.anchored.by_page.get(&index) else {
            return Vec::new();
        };
        let geometry = self.paginator.master(index, &self.slot).geometry;
        let (origin, width) = if self.tier().spanning {
            (geometry.content_origin(), geometry.content_size().0)
        } else {
            (geometry.column_origin(self.column), geometry.measure())
        };
        let column = Rect {
            x: origin.0,
            y: origin.1,
            w: width,
            h: self.height,
        };
        anchored
            .iter()
            .map(|at| &self.anchored.all[*at])
            .filter(|image| image.wrap != WrapFlow::Auto)
            .map(|image| (image, image.rect(geometry)))
            .filter(|(_, rect)| rect.meets(column))
            .map(|(image, rect)| Hole {
                rect: rect.within(origin),
                wrap: image.wrap,
                shape: image.shape.as_ref(),
            })
            .collect()
    }

    /// The bands a paragraph that starts at `top` in the column being
    /// filled is set in, and `None` where no image reaches them.
    ///
    /// An image covers whole bands. The flow snaps it to the
    /// paragraph's own leading, so a line is either set beside it or
    /// clear of it. A band it covers the whole of is a band nothing
    /// is set in, and the paragraph goes on below it.
    ///
    /// `hung` is where the markers of the list items the paragraph
    /// opens start, from its leading edge.
    fn profile(
        &self,
        top: f32,
        reflow: &Reflow,
        opening: bool,
        hung: Option<f32>,
    ) -> Option<Profile> {
        // The bands are the paragraph's own, from its leading edge.
        // The images are the column's. One of them has to move.
        let holes: Vec<Hole> = self
            .holes()
            .into_iter()
            .map(|hole| Hole {
                rect: hole.rect.within((reflow.setting.x, 0.0)),
                ..hole
            })
            .collect();
        if holes.is_empty() {
            return None;
        }
        let leading = reflow.leading.max(1.0);
        let narrowest = reflow.shaped.style().size;
        // The band every band past the profile is set in, and the one
        // the profile has to list a band that differs from.
        let plain = reflow.base.rest();
        let cap = reflow.setting.cap.as_ref().filter(|_| opening);
        // An initial letter is one box over the bands it is sunk
        // over, so it goes where they are clear for the whole of its
        // height, with room for a line beside it. Its own bands are
        // read against the plain band and give up its column. The
        // bands the profile was built with hold the column the letter
        // takes with nothing in the way, which is not the column to
        // read here.
        let column = |cap: &Cap, y: f32| {
            let bottom = y + cap.lines as f32 * leading;
            clear(plain, &holes, y, bottom, cap.reserved + narrowest)
                .first()
                .map(|(origin, _)| *origin)
        };
        let mut letter = None;
        let mut spans = Vec::new();
        let mut gaps = Vec::new();
        // Whether an image reached any band at all, and how far down
        // the profile has to be listed: the shorter it is, the fewer
        // states the break has to keep.
        let mut reached = false;
        let mut listed = (0, 0);
        let (mut y, mut band, mut gap) = (top, 0, 0.0);
        while y < self.height {
            let sunk = cap.filter(|cap| band < cap.lines);
            // A first-line indent and a drop cap belong to the line
            // the paragraph opens on. What is left of it opens on a
            // band like any other.
            let base = match (opening, sunk) {
                (_, Some(_)) => plain,
                (true, None) => reflow.base.at(band),
                (false, None) => plain,
            };
            let mut free = match hung.filter(|_| opening && band == 0 && sunk.is_none()) {
                Some(hung) => clear_hung(base, hung, &holes, y, y + leading, narrowest),
                None => clear(base, &holes, y, y + leading, narrowest),
            };
            if let Some(cap) = sunk {
                // Where the letter goes is settled on the first of
                // its bands and held for the rest of them.
                let at = match letter {
                    Some(at) => Some(at),
                    None => column(cap, y),
                };
                free = match at {
                    Some(at) => taking(free, at, cap.reserved, narrowest),
                    None => Vec::new(),
                };
                letter = at;
            }
            let Some((last, rest)) = free.split_last() else {
                // Nothing is set in a band an image covers the whole
                // of, so the next band is the first one under it. A
                // box covers every band down to its foot; a contour
                // may narrow at any of them, so it is asked again one
                // band down.
                let below = holes
                    .iter()
                    .filter(|hole| hole.covering(y, y + leading).is_some())
                    .fold(y, |below: f32, hole| {
                        below.max(match hole.shape {
                            None => hole.rect.bottom(),
                            Some(_) => y + leading,
                        })
                    });
                if below <= y {
                    break;
                }
                gap += below - y;
                y = below;
                continue;
            };
            for (origin, width) in rest {
                spans.push(Span {
                    origin: *origin,
                    width: *width,
                    ends_band: false,
                });
            }
            spans.push(Span::band(last.0, last.1));
            let above = std::mem::take(&mut gap);
            gaps.push(above);
            band += 1;
            y += leading;
            let alone = |span: Span| free.len() == 1 && *last == (span.origin, span.width);
            let unmoved = match (opening, sunk) {
                (true, Some(_)) => alone(reflow.base.at(band - 1)),
                _ => alone(base),
            };
            reached = reached || above != 0.0 || !unmoved;
            if above != 0.0 || !alone(plain) {
                listed = (spans.len(), band);
            }
        }
        // A box down to the foot of the column leaves no band under
        // it, so the line after the last band goes past the foot.
        if gap != 0.0 {
            gaps.push(gap);
            reached = true;
            listed = (spans.len(), band + 1);
        }
        if !reached {
            return None;
        }
        spans.truncate(listed.0);
        gaps.truncate(listed.1);
        Some(Profile {
            measure: Measure::new(spans, plain),
            gaps,
            letter,
        })
    }
}

/// One image as the column being filled sees it: the rectangle it
/// covers, the contour inside that rectangle where it has one, and
/// which side of it the prose sets on.
#[derive(Debug, Clone, Copy)]
struct Hole<'a> {
    rect: Rect,
    wrap: WrapFlow,
    shape: Option<&'a Shape>,
}

impl Hole<'_> {
    /// What the hole covers of the band between `top` and `bottom`,
    /// and `None` where it reaches none of it.
    ///
    /// A box covers the same stretch at every height. A contour is
    /// read band by band, over the band grown by the shape margin,
    /// and what it gives back is grown by it too.
    fn covering(&self, top: f32, bottom: f32) -> Option<(f32, f32)> {
        let Some(shape) = self.shape else {
            return (self.rect.bottom() > top && self.rect.y < bottom)
                .then(|| (self.rect.x, self.rect.right()));
        };
        let (from, to) = (
            top - self.rect.y - shape.margin,
            bottom - self.rect.y + shape.margin,
        );
        let (left, right) = scanline(&shape.rings, from, to)?;
        Some((
            self.rect.x + left - shape.margin,
            self.rect.x + right + shape.margin,
        ))
    }
}

/// The bands a paragraph is set in beside an image: what each of them
/// is left of the measure, and the space above a band that had to
/// move past an image that covers the whole of it.
struct Profile {
    measure: Measure,
    gaps: Vec<f32>,
    /// Where the initial letter goes, from the paragraph's leading
    /// edge, when an image moved it.
    letter: Option<f32>,
}

impl Profile {
    /// The bands with nothing in the way: what a paragraph moved off
    /// the page it was narrowed on is set to instead.
    fn plain(reflow: &Reflow, opening: bool) -> Profile {
        Profile {
            measure: if opening {
                reflow.base.clone()
            } else {
                Measure::new(Vec::new(), reflow.base.rest())
            },
            gaps: Vec::new(),
            letter: None,
        }
    }
}

/// A rectangle on the page, in page coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn right(self) -> f32 {
        self.x + self.w
    }

    fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Whether two rectangles share any area.
    fn meets(self, other: Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The same rectangle read from another origin.
    fn within(self, origin: (f32, f32)) -> Rect {
        Rect {
            x: self.x - origin.0,
            y: self.y - origin.1,
            ..self
        }
    }
}

/// What one band has left of it where the images on its page cover
/// it: the stretches the prose can be set in, in reading order.
///
/// A stretch narrower than `narrowest` holds nothing worth setting
/// and is not one.
fn clear(base: Span, holes: &[Hole], top: f32, bottom: f32, narrowest: f32) -> Vec<(f32, f32)> {
    let mut free = vec![(base.origin, base.origin + base.width)];
    for hole in holes {
        let Some((left, right)) = hole.covering(top, bottom) else {
            continue;
        };
        free = free
            .iter()
            .flat_map(|(start, end)| {
                if right <= *start || left >= *end {
                    return vec![(*start, *end)];
                }
                match hole.wrap {
                    WrapFlow::Auto => vec![(*start, *end)],
                    WrapFlow::Start => vec![(*start, end.min(left))],
                    WrapFlow::End => vec![(start.max(right), *end)],
                    WrapFlow::Both => vec![(*start, end.min(left)), (start.max(right), *end)],
                }
            })
            .filter(|(start, end)| end - start >= narrowest)
            .collect();
    }
    free.into_iter()
        .map(|(start, end)| (start, end - start))
        .collect()
}

/// What one band has left of it for the line a list item opens on,
/// where the item's markers start at `hung`. The markers keep their
/// distance from the text, so an image in the way of either moves both.
fn clear_hung(
    base: Span,
    hung: f32,
    holes: &[Hole],
    top: f32,
    bottom: f32,
    narrowest: f32,
) -> Vec<(f32, f32)> {
    let before = base.origin - hung;
    if before <= 0.0 {
        return clear(base, holes, top, bottom, narrowest);
    }
    let mut free = clear(
        Span::band(hung, base.width + before),
        holes,
        top,
        bottom,
        narrowest,
    );
    while let Some(&(start, width)) = free.first() {
        if start == hung && width - before >= narrowest {
            return clear(base, holes, top, bottom, narrowest);
        }
        if width - before >= narrowest {
            free[0] = (start + before, width - before);
            break;
        }
        free.remove(0);
    }
    free
}

/// The leftmost and rightmost point a contour reaches between two
/// heights, and `None` where it reaches neither.
///
/// This is what turns a polygon into per-band spans. A ring is closed
/// by its own first point. An edge inside the band contributes its
/// ends, and one that crosses the band's edge contributes where it
/// crosses.
fn scanline(rings: &[Vec<[f32; 2]>], top: f32, bottom: f32) -> Option<(f32, f32)> {
    let mut reach: Option<(f32, f32)> = None;
    let mut widen = |x: f32| {
        reach = Some(match reach {
            None => (x, x),
            Some((left, right)) => (left.min(x), right.max(x)),
        });
    };
    for ring in rings {
        for (at, from) in ring.iter().enumerate() {
            let to = ring[(at + 1) % ring.len()];
            let (above, below) = (from[1].min(to[1]), from[1].max(to[1]));
            if below < top || above > bottom {
                continue;
            }
            for point in [*from, to] {
                if point[1] >= top && point[1] <= bottom {
                    widen(point[0]);
                }
            }
            for cut in [top, bottom] {
                if cut > above && cut < below {
                    let along = (cut - from[1]) / (to[1] - from[1]);
                    widen(from[0] + along * (to[0] - from[0]));
                }
            }
        }
    }
    reach
}

/// What one band has left of it once the initial letter beside it
/// takes its column, which starts at `at` and runs `reserved` wide.
///
/// A stretch narrower than `narrowest` holds nothing worth setting
/// and is not one.
fn taking(free: Vec<(f32, f32)>, at: f32, reserved: f32, narrowest: f32) -> Vec<(f32, f32)> {
    free.into_iter()
        .map(|(origin, width)| {
            let end = origin + width;
            if at + reserved <= origin || at >= end {
                return (origin, width);
            }
            let start = origin.max(at + reserved);
            (start, end - start)
        })
        .filter(|(_, width)| *width >= narrowest)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LayoutOutput;
    use crate::content::{Attributes, Block, NodeId};
    use crate::layout::testing::{
        ContentLine, Run, body_size, book_of, content_items, content_lines, folio_size, heading,
        image, image_of, long_prose, master, page_geometry, paginate_styled, painted, paragraph,
        png, quote, rects, registry, right_edge, section, styled, styled_geometry, tagged_lines,
        tagged_prose, with_image, with_images,
    };
    use crate::pages::{DrawItem, Page, Side};
    use crate::style::Situation;

    /// The image the tests anchor is 144pt square.
    const IMAGE: f32 = 144.0;

    /// The quotation the tests of anchored blocks lift out of the
    /// flow. Every word of it is one the prose never uses.
    fn lifted_quote() -> Block {
        quote(vec![paragraph(&"lilliputian ".repeat(40))])
    }

    /// The content lines of a page that the lifted quotation paints.
    fn quoted_lines(page: &Page) -> Vec<ContentLine<'_>> {
        content_lines(page)
            .into_iter()
            .filter(|(_, runs)| runs.iter().any(|run| run.2.contains("lilliputian")))
            .collect()
    }

    /// Acceptance: a block with `position: absolute; bottom: 1in;
    /// left: 0.5in` sits at that place on the page its anchor landed
    /// on, and takes no height in the flow.
    #[test]
    fn an_absolute_block_sits_at_its_insets_on_the_page_its_anchor_landed_on() {
        // A lifted block is still the sibling of the paragraph after
        // it, so `p + p` no longer matches that paragraph. Both books
        // turn the indent off, so the flow is the only difference.
        let css = "blockquote { position: absolute; bottom: 1in; left: 0.5in; margin: 0; \
                   background-color: #eeeeee } p { text-indent: 0 }";
        // Which paragraph opens the second page with no quotation in
        // the book: the quotation goes above that one.
        let bare = paginate_styled(css, vec![section(tagged_prose(30))]);
        assert!(bare.len() > 1, "one page proves nothing here");
        let above = tagged_lines(&bare[0]);
        let opening = tagged_lines(&bare[1])
            .into_iter()
            .find(|tag| !above.contains(tag))
            .expect("a paragraph opens on the second page");
        let nth: usize = opening
            .trim_start_matches('p')
            .parse()
            .expect("the tag counts the paragraph");
        let mut blocks = tagged_prose(30);
        blocks.insert(nth, lifted_quote());
        let lifted = paginate_styled(css, vec![section(blocks)]);

        assert_eq!(lifted.len(), bare.len(), "the page count moved");
        // The engine places a book with an anchored box one paragraph
        // at a time. It adds the same heights in another order, so a
        // baseline can differ in its last bit.
        let same = |one: &[ContentLine<'_>], other: &[ContentLine<'_>]| {
            one.len() == other.len()
                && one.iter().zip(other).all(|((a, runs), (b, others))| {
                    (a - b).abs() < 1e-3
                        && runs.len() == others.len()
                        && runs
                            .iter()
                            .zip(others)
                            .all(|(run, other)| (run.0 - other.0).abs() < 1e-3 && run.2 == other.2)
                })
        };
        for (page, plain) in lifted.iter().zip(&bare) {
            let prose: Vec<ContentLine<'_>> = content_lines(page)
                .into_iter()
                .filter(|(_, runs)| !runs.iter().any(|run| run.2.contains("lilliputian")))
                .collect();
            assert!(
                same(&prose, &content_lines(plain)),
                "page {}: the block took room in the flow",
                page.number,
            );
        }
        assert!(
            quoted_lines(&lifted[0]).is_empty(),
            "the block did not wait for its page"
        );
        let page = &lifted[1];
        let lines = quoted_lines(page);
        assert!(!lines.is_empty(), "the block is not on its anchor's page");

        let geometry = page_geometry(css, page);
        let (left, top) = geometry.content_origin();
        let foot = top + geometry.content_size().1;
        let boxes = rects(page);
        let [(x, y, _, h, _)] = boxes.as_slice() else {
            panic!("the block paints one box: {boxes:?}");
        };
        assert!(
            (x - (left + 36.0)).abs() < 1e-3,
            "the box starts {} in",
            x - left
        );
        assert!(
            (y + h - (foot - 72.0)).abs() < 1e-3,
            "the box ends {} above the foot",
            foot - y - h,
        );
        for (baseline, runs) in &lines {
            assert!(*baseline > *y && *baseline < y + h, "a line at {baseline}");
            assert!(runs[0].0 >= left + 36.0 - 1e-3, "a line at {baseline}");
        }
    }

    /// Acceptance: the text of an absolute block breaks to the width
    /// its insets leave. With both set, that is the width between
    /// them. With one set, it is the width from that one to the far
    /// edge.
    #[test]
    fn an_absolute_blocks_text_breaks_to_the_width_its_insets_leave() {
        let geometry = master(Situation::First(Side::Recto)).geometry;
        let (left, width) = (geometry.content_origin().0, geometry.content_size().0);
        for (insets, from, to) in [
            ("left: 72pt; right: 72pt", left + 72.0, left + width - 72.0),
            ("left: 50%", left + width / 2.0, left + width),
            ("right: 25%", left, left + width * 0.75),
        ] {
            let css = format!("blockquote {{ position: absolute; top: 50%; {insets}; margin: 0 }}");
            let pages = paginate_styled(
                &css,
                vec![section(vec![lifted_quote(), paragraph("after")])],
            );
            let page = &pages[0];
            let lines = quoted_lines(page);
            assert!(lines.len() > 2, "{insets}: {} lines", lines.len());
            let mut widest = f32::MIN;
            for (baseline, runs) in &lines {
                let end = right_edge(page, *baseline);
                assert!(
                    runs[0].0 >= from - 1e-3,
                    "{insets}: a line starts at {}",
                    runs[0].0
                );
                assert!(
                    end <= to + 1e-3,
                    "{insets}: a line ends at {end}, past {to}"
                );
                widest = widest.max(end);
            }
            assert!(
                widest > to - 72.0,
                "{insets}: the lines stop at {widest}, well short of {to}",
            );
        }
    }

    /// Part: a percentage on an inset is a percentage of the page area.
    /// It uses the width for `left` and `right`, and the height for
    /// `top` and `bottom`. This is true for a block and for an image.
    #[test]
    fn a_percentage_inset_measures_the_page_area() {
        let geometry = master(Situation::First(Side::Recto)).geometry;
        let (left, top) = geometry.content_origin();
        let (width, height) = geometry.content_size();
        let near = |a: f32, b: f32| (a - b).abs() < 1e-2;

        let css = "blockquote { position: absolute; top: 25%; left: 10%; right: 40%; \
                   margin: 0; background-color: #eeeeee }";
        let pages = paginate_styled(css, vec![section(vec![lifted_quote(), paragraph("after")])]);
        let boxes = rects(&pages[0]);
        let [(x, y, w, _, _)] = boxes.as_slice() else {
            panic!("the block paints one box: {boxes:?}");
        };
        assert!(near(*x, left + width * 0.1), "the box starts at {x}");
        assert!(near(*y, top + height * 0.25), "the box opens at {y}");
        assert!(near(*w, width * 0.5), "the box is {w} wide");

        let output = with_image(
            "img { position: absolute; bottom: 10%; right: 50% }",
            vec![section(
                std::iter::once(image()).chain(long_prose(2)).collect(),
            )],
        );
        let [(x, y, _, _)] = painted(&output.pages[0])[..] else {
            panic!("one image is painted");
        };
        assert!(
            near(x, left + width * 0.5 - IMAGE),
            "the image starts at {x}"
        );
        assert!(
            near(y, top + height * 0.9 - IMAGE),
            "the image opens at {y}"
        );
    }

    /// An absolute block with a `wrap-flow` other than `auto` is an
    /// exclusion the way an image is: the prose of its page wraps
    /// beside it.
    #[test]
    fn prose_sets_beside_an_absolute_block_that_asks_for_it() {
        let css = "blockquote { position: absolute; top: 0; left: 0; right: 50%; margin: 0; \
                   wrap-flow: end; background-color: #eeeeee }";
        let pages = paginate_styled(
            css,
            vec![section(
                std::iter::once(lifted_quote())
                    .chain(long_prose(8))
                    .collect(),
            )],
        );
        let page = &pages[0];
        let boxes = rects(page);
        let [(x, y, w, h, _)] = boxes.as_slice() else {
            panic!("the block paints one box: {boxes:?}");
        };
        let left = master(Situation::First(Side::Recto))
            .geometry
            .content_origin()
            .0;
        let prose: Vec<ContentLine<'_>> = content_lines(page)
            .into_iter()
            .filter(|(_, runs)| !runs.iter().any(|run| run.2.contains("lilliputian")))
            .collect();
        let beside: Vec<&ContentLine<'_>> = prose
            .iter()
            .filter(|(baseline, _)| *baseline > *y && *baseline < y + h)
            .collect();
        assert!(!beside.is_empty(), "no line is set beside the block");
        for (baseline, runs) in beside {
            assert!(
                runs[0].0 >= x + w - 1e-3,
                "the line at {baseline} starts at {}, over the block",
                runs[0].0,
            );
        }
        assert!(
            prose
                .iter()
                .any(|(_, runs)| (runs[0].0 - left).abs() < 1e-3),
            "no line under the block runs the full measure",
        );
    }

    /// An `h2` with the words `value`.
    fn h2(value: &str) -> Block {
        let Block::Heading { inlines, .. } = heading(value) else {
            unreachable!("`heading` makes a heading");
        };
        Block::Heading {
            id: NodeId::UNASSIGNED,
            level: crate::content::HeadingLevel::H2,
            inlines,
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    /// Whether two pages set the same lines, to the last thousandth of
    /// a point.
    fn same_lines(one: &[ContentLine<'_>], other: &[ContentLine<'_>]) -> bool {
        one.len() == other.len()
            && one.iter().zip(other).all(|((a, runs), (b, others))| {
                (a - b).abs() < 1e-3
                    && runs.len() == others.len()
                    && runs
                        .iter()
                        .zip(others)
                        .all(|(run, other)| (run.0 - other.0).abs() < 1e-3 && run.2 == other.2)
            })
    }

    /// Acceptance: `h2::before { content: "\2766"; position: absolute;
    /// top: 0; left: 0 }` puts an ornament at the top left of the
    /// content box on each page an h2 lands on, and takes no height in
    /// the flow.
    #[test]
    fn an_absolute_generated_box_sits_at_the_top_left_of_each_page_its_heading_lands_on() {
        const FLEURON: &str = "\u{2766}";
        let css = "h2::before { content: \"\\2766\"; position: absolute; top: 0; left: 0 }";
        let book = || {
            vec![
                section(vec![h2("First"), paragraph("one")]),
                section(vec![h2("Second"), paragraph("two")]),
            ]
        };
        let bare = paginate_styled("", book());
        let marked = paginate_styled(css, book());
        assert_eq!(marked.len(), bare.len(), "the page count moved");

        let mut carrying = 0;
        for (page, plain) in marked.iter().zip(&bare) {
            let lines = content_lines(page);
            let ornaments: Vec<(f32, f32)> = lines
                .iter()
                .flat_map(|(baseline, runs)| {
                    runs.iter()
                        .filter(|run| run.2 == FLEURON)
                        .map(move |run| (*baseline, run.0))
                })
                .collect();
            // The ornament can share a baseline with the heading, so
            // its runs come out of the lines rather than whole lines.
            let prose: Vec<ContentLine<'_>> = lines
                .iter()
                .map(|(baseline, runs)| {
                    let runs = runs.iter().filter(|run| run.2 != FLEURON).copied();
                    (*baseline, runs.collect::<Vec<_>>())
                })
                .filter(|(_, runs)| !runs.is_empty())
                .collect();
            assert!(
                same_lines(&prose, &content_lines(plain)),
                "page {}: the ornament took room in the flow",
                page.number,
            );
            let titled = prose
                .iter()
                .any(|(_, runs)| runs.iter().any(|run| run.2 == "First" || run.2 == "Second"));
            if !titled {
                assert!(ornaments.is_empty(), "page {}: {ornaments:?}", page.number);
                continue;
            }
            carrying += 1;
            let [(baseline, x)] = ornaments[..] else {
                panic!("page {}: one ornament, not {ornaments:?}", page.number);
            };
            let (left, top) = master(Situation::First(page.side))
                .geometry
                .content_origin();
            assert!(
                (x - left).abs() < 1e-3,
                "page {}: the ornament starts at {x}",
                page.number
            );
            assert!(
                baseline > top && baseline < top + 36.0,
                "page {}: the ornament's baseline is {} under the top",
                page.number,
                baseline - top,
            );
        }
        assert_eq!(carrying, 2, "each heading lands on a page of its own");
    }

    /// Acceptance: a generated box with `position: absolute` and
    /// `wrap-flow: end` holds the prose off it, the same as a block
    /// with those properties.
    #[test]
    fn prose_sets_beside_a_generated_box_as_it_does_beside_a_block() {
        const PLACED: &str = "position: absolute; top: 0; left: 0; right: 50%; margin: 0; \
                              wrap-flow: end; background-color: #eeeeee";
        let words = "lilliputian ".repeat(40);
        let block = paginate_styled(
            &format!("p {{ text-indent: 0 }} p:first-child {{ {PLACED} }}"),
            vec![section(
                std::iter::once(paragraph(&words))
                    .chain(long_prose(8))
                    .collect(),
            )],
        );
        let generated = paginate_styled(
            &format!("p {{ text-indent: 0 }} section::before {{ content: \"{words}\"; {PLACED} }}"),
            vec![section(long_prose(8))],
        );
        assert_eq!(generated.len(), block.len(), "the page count moved");

        let page = &generated[0];
        let boxes = rects(page);
        let [(x, y, w, h, _)] = boxes.as_slice() else {
            panic!("the generated box paints one box: {boxes:?}");
        };
        let [(bx, by, bw, bh, _)] = rects(&block[0])[..] else {
            panic!("the block paints one box");
        };
        for (one, other) in [(*x, bx), (*y, by), (*w, bw), (*h, bh)] {
            assert!(
                (one - other).abs() < 1e-3,
                "the generated box is at {:?}, the block at {:?}",
                (x, y, w, h),
                (bx, by, bw, bh),
            );
        }

        let prose = |page| -> Vec<ContentLine<'_>> {
            content_lines(page)
                .into_iter()
                .filter(|(_, runs)| !runs.iter().any(|run| run.2.contains("lilliputian")))
                .collect()
        };
        assert!(
            same_lines(&prose(page), &prose(&block[0])),
            "the prose sets differently beside the generated box",
        );
        let beside: Vec<ContentLine<'_>> = prose(page)
            .into_iter()
            .filter(|(baseline, _)| *baseline > *y && *baseline < y + h)
            .collect();
        assert!(!beside.is_empty(), "no line is set beside the box");
        for (baseline, runs) in beside {
            assert!(
                runs[0].0 >= x + w - 1e-3,
                "the line at {baseline} starts at {}, over the box",
                runs[0].0,
            );
        }
    }

    /// The baselines of the prose lines a page sets below `top`,
    /// leaving out the lifted quotation's own.
    fn prose_below(page: &Page, top: f32) -> Vec<f32> {
        content_lines(page)
            .into_iter()
            .filter(|(baseline, runs)| {
                *baseline > top && !runs.iter().any(|run| run.2.contains("lilliputian"))
            })
            .map(|(baseline, _)| baseline)
            .collect()
    }

    /// The sheet the tests of a block at the foot of the column lift
    /// the quotation with.
    const AT_THE_FOOT: &str = "blockquote { position: absolute; bottom: 0; left: 0; right: 0; \
                               margin: 0; wrap-flow: both; background-color: #eeeeee }";

    /// Acceptance: no line sets over a box with a `wrap-flow` other
    /// than `auto` that reaches the foot of the column. The text
    /// continues on the next page.
    #[test]
    fn prose_does_not_set_over_a_block_at_the_foot_of_the_column() {
        let pages = paginate_styled(
            AT_THE_FOOT,
            vec![section(
                std::iter::once(lifted_quote())
                    .chain(long_prose(16))
                    .collect(),
            )],
        );
        assert!(pages.len() > 1, "the prose does not reach the foot");
        let boxes = rects(&pages[0]);
        let [(_, y, _, _, _)] = boxes.as_slice() else {
            panic!("the block paints one box: {boxes:?}");
        };
        assert!(
            !prose_below(&pages[0], 0.0).is_empty(),
            "no prose on the first page",
        );
        assert_eq!(
            prose_below(&pages[0], *y),
            Vec::<f32>::new(),
            "lines set over the block",
        );
        assert!(
            !prose_below(&pages[1], 0.0).is_empty(),
            "the prose does not continue on the next page",
        );
    }

    /// Acceptance: a paragraph that starts in the space a box at the
    /// foot of the column covers starts on the next page.
    #[test]
    fn a_paragraph_that_starts_beside_a_block_at_the_foot_starts_on_the_next_page() {
        let one_line = || paragraph("my father had a small estate");
        let pages = paginate_styled(
            &format!("{AT_THE_FOOT} p {{ text-indent: 0 }}"),
            vec![section(
                std::iter::once(lifted_quote())
                    .chain(std::iter::repeat_with(one_line).take(80))
                    .collect(),
            )],
        );
        assert!(pages.len() > 1, "the prose does not reach the foot");
        let boxes = rects(&pages[0]);
        let [(_, y, _, _, _)] = boxes.as_slice() else {
            panic!("the block paints one box: {boxes:?}");
        };
        assert_eq!(
            prose_below(&pages[0], *y),
            Vec::<f32>::new(),
            "paragraphs started over the block",
        );
    }

    /// Acceptance: an anchored image at the foot of the column holds
    /// the text off in the same way. Its margin takes the rest of the
    /// measure, so no band beside it has room for a line.
    #[test]
    fn prose_does_not_set_over_an_image_at_the_foot_of_the_column() {
        let measure = master(Situation::First(Side::Recto)).geometry.measure();
        let output = with_image(
            &format!(
                "img {{ position: absolute; bottom: 0; left: 0; margin-right: {}pt; \
                 wrap-flow: end }}",
                measure - IMAGE,
            ),
            vec![section(
                std::iter::once(image()).chain(long_prose(16)).collect(),
            )],
        );
        assert!(output.pages.len() > 1, "the prose does not reach the foot");
        let images = painted(&output.pages[0]);
        let [(_, y, _, _)] = images.as_slice() else {
            panic!("the first page paints one image: {images:?}");
        };
        assert_eq!(
            prose_below(&output.pages[0], *y),
            Vec::<f32>::new(),
            "lines set over the image",
        );
    }

    /// An anchored image takes the size the sheet gives it, and a
    /// percentage measures the page area.
    #[test]
    fn an_anchored_image_takes_the_size_the_sheet_gives_it() {
        let (width, height) = master(Situation::First(Side::Recto))
            .geometry
            .content_size();
        let size = |css: &str| {
            let output = with_image(
                &format!("img {{ position: absolute; top: 0; left: 0; {css} }}"),
                vec![section(
                    std::iter::once(image()).chain(long_prose(2)).collect(),
                )],
            );
            let images = painted(&output.pages[0]);
            let [(_, _, w, h)] = images.as_slice() else {
                panic!("the first page paints one image: {images:?}");
            };
            (*w, *h)
        };
        assert_eq!(size(""), (IMAGE, IMAGE));
        assert_eq!(size("width: 72pt"), (72.0, 72.0));
        assert_eq!(size("width: 50%; height: 20pt"), (width / 2.0, 20.0));
        assert_eq!(size("max-height: 10%"), (height / 10.0, height / 10.0));
    }

    /// An image anchored inside an absolute block lands on the page
    /// that block lands on.
    #[test]
    fn an_image_anchored_inside_an_absolute_block_lands_with_it() {
        let mut blocks = tagged_prose(30);
        blocks.insert(20, quote(vec![paragraph("lilliputian words"), image()]));
        let output = with_image(
            "blockquote, img { position: absolute; top: 0; left: 0 }",
            vec![section(blocks)],
        );
        let at = output
            .pages
            .iter()
            .position(|page| !quoted_lines(page).is_empty())
            .expect("the block is painted");
        assert!(at > 0, "the anchor landed on the first page");
        let images: Vec<usize> = output
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| !painted(page).is_empty())
            .map(|(index, _)| index)
            .collect();
        assert_eq!(images, vec![at], "the image did not land with its block");
    }

    /// The page that paints the image `width` points wide.
    fn page_of_image(pages: &[Page], width: f32) -> Option<usize> {
        pages.iter().position(|page| {
            painted(page)
                .iter()
                .any(|(_, _, w, _)| (w - width).abs() < 1e-3)
        })
    }

    /// The page where the paragraph tagged `tag` starts.
    fn page_of_paragraph(pages: &[Page], tag: &str) -> Option<usize> {
        pages
            .iter()
            .position(|page| tagged_lines(page).iter().any(|line| line == tag))
    }

    /// The images of the illustrated book, by url.
    struct Files(Vec<(String, Vec<u8>)>);

    impl crate::images::ImageLoader for Files {
        fn load(&self, url: &str) -> Option<Vec<u8>> {
            self.0
                .iter()
                .find(|(name, _)| name == url)
                .map(|(_, bytes)| bytes.clone())
        }
    }

    /// A book of 60 paragraphs with an image above every third one.
    /// Each image is 4px wider than the one before it, so a page tells
    /// them apart. Each pair is the paragraph under an image and the
    /// width of that image in points.
    fn illustrated_book() -> (Book, Files, Vec<(usize, f32)>) {
        let under: Vec<usize> = (1..60).step_by(3).collect();
        let sides: Vec<u32> = (0..under.len() as u32).map(|at| 192 + 4 * at).collect();
        let mut blocks = tagged_prose(60);
        for (at, index) in under.iter().enumerate().rev() {
            blocks.insert(*index, image_of(&format!("{at}.png"), Vec::new()));
        }
        let files = Files(
            sides
                .iter()
                .enumerate()
                .map(|(at, side)| (format!("{at}.png"), png(*side, *side)))
                .collect(),
        );
        let widths = under
            .into_iter()
            .zip(sides.iter().map(|side| *side as f32 * 0.75))
            .collect();
        (book_of(vec![section(blocks)]), files, widths)
    }

    /// The illustrated book laid out under `css`, and how many times
    /// the flow that paints set a paragraph again.
    fn paginate_illustrated(css: &str, book: &Book, files: &Files) -> (Vec<Page>, u32) {
        let styles = styled(css, book);
        let assets = crate::images::Assets::probe(book, &styles, files);
        let paginator = Paginator::with_assets(registry(), &styles, &assets);
        let pages = paginator.paginate(book);
        (pages, paginator.rebreaks())
    }

    /// The sheet that sets the prose of the illustrated book beside
    /// its images.
    const BESIDE: &str =
        "img { position: absolute; top: 0; left: 0; margin-right: 12pt; wrap-flow: end }";

    /// Acceptance: in a book with an image above every third
    /// paragraph, each image lands on the page where its paragraph
    /// starts, or on the next page where it pushed that paragraph off.
    #[test]
    fn every_image_in_an_illustrated_book_lands_with_its_paragraph() {
        let (book, files, images) = illustrated_book();
        let (pages, _) = paginate_illustrated(BESIDE, &book, &files);
        for (index, width) in images {
            let image = page_of_image(&pages, width).expect("every image is painted");
            let paragraph =
                page_of_paragraph(&pages, &format!("p{index:02}")).expect("every paragraph is set");
            assert!(
                image == paragraph || image == paragraph + 1,
                "the image {width}pt wide is on page {image}, and its paragraph starts on \
                 page {paragraph}",
            );
        }
    }

    /// Acceptance: a book whose boxes all have `wrap-flow: auto` breaks
    /// its lines as often as it did before. No paragraph is set again,
    /// and every page sets the lines of the same book with no image in
    /// it.
    #[test]
    fn a_book_of_boxes_that_wrap_nothing_sets_no_paragraph_again() {
        let css =
            "p { text-indent: 0 } img { position: absolute; top: 0; left: 0; wrap-flow: auto }";
        let (book, files, _) = illustrated_book();
        let (pages, rebreaks) = paginate_illustrated(css, &book, &files);
        assert_eq!(rebreaks, 0, "a paragraph was set again");
        let bare = paginate_styled(css, vec![section(tagged_prose(60))]);
        assert_eq!(pages.len(), bare.len(), "the page count moved");
        for (page, plain) in pages.iter().zip(&bare) {
            assert!(
                same_lines(&content_lines(page), &content_lines(plain)),
                "page {} sets other lines",
                page.number,
            );
        }
    }

    /// Acceptance: an illustrated book with many wrapping images lays
    /// out the same way twice.
    #[test]
    fn a_book_with_many_wrapping_images_lays_out_the_same_way_twice() {
        let (book, files, _) = illustrated_book();
        let (once, _) = paginate_illustrated(BESIDE, &book, &files);
        let (twice, _) = paginate_illustrated(BESIDE, &book, &files);
        assert_eq!(
            serde_json::to_string(&once).expect("the pages encode"),
            serde_json::to_string(&twice).expect("the pages encode"),
        );
    }

    /// Acceptance: a box that pushes its own anchor onto the next page
    /// lands on that next page, and the layout ends. The paragraph
    /// starts where it starts with no box in the book.
    #[test]
    fn an_image_that_pushes_its_paragraph_off_its_page_lands_on_the_next_page() {
        let css = "p { text-indent: 0 }";
        let bare = paginate_styled(css, vec![section(tagged_prose(30))]);
        // The image covers the whole measure for 144pt, which is more
        // lines than the paragraph starts from the foot.
        let (page, tag) = bare
            .iter()
            .enumerate()
            .find_map(|(page, content)| {
                let lines = tagged_lines(content);
                (1..lines.len())
                    .find(|at| lines[*at] != lines[at - 1] && lines.len() - at <= 4)
                    .map(|at| (page, lines[at].clone()))
            })
            .expect("a paragraph starts in the last lines of a page");
        let index: usize = tag
            .trim_start_matches('p')
            .parse()
            .expect("the tag counts the paragraph");
        let mut blocks = tagged_prose(30);
        blocks.insert(index, image());
        let measure = master(Situation::First(Side::Recto)).geometry.measure();
        let output = with_image(
            &format!(
                "{css} img {{ position: absolute; top: 0; left: 0; margin-right: {}pt; \
                 wrap-flow: end }}",
                measure - IMAGE,
            ),
            vec![section(blocks)],
        );
        assert_eq!(
            page_of_paragraph(&output.pages, &tag),
            Some(page),
            "the paragraph moved"
        );
        assert_eq!(page_of_image(&output.pages, IMAGE), Some(page + 1));
    }

    /// An RGBA PNG two inches square at 96dpi, opaque where
    /// `covered` says so.
    fn alpha_png(covered: impl Fn(u32, u32) -> bool) -> Vec<u8> {
        let side = 192u32;
        let mut pixels = Vec::with_capacity((side * side * 4) as usize);
        for y in 0..side {
            for x in 0..side {
                pixels.extend([0x22, 0x33, 0x44, if covered(x, y) { 0xFF } else { 0 }]);
            }
        }
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, side, side);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("the header writes");
        writer.write_image_data(&pixels).expect("the pixels write");
        writer.finish().expect("the file closes");
        bytes
    }

    /// The book the anchoring tests lay out, over an image whose
    /// alpha `covered` decides, with the contour traced first.
    fn traced(css: &str, covered: impl Fn(u32, u32) -> bool + Sync + 'static) -> Vec<Page> {
        struct Alpha(Vec<u8>);
        impl crate::images::ImageLoader for Alpha {
            fn load(&self, url: &str) -> Option<Vec<u8>> {
                (url == "image.png").then(|| self.0.clone())
            }
        }
        let book = book_of(vec![section(
            std::iter::once(image()).chain(long_prose(6)).collect(),
        )]);
        let styles = styled(css, &book);
        let assets = crate::images::Assets::probe(&book, &styles, &Alpha(alpha_png(covered)));
        let mut contours = crate::images::Contours::none();
        contours.update(&book, &styles, &assets);
        Paginator::with_contours(registry(), &styles, &assets, &contours).paginate(&book)
    }

    /// Where every line of a page starts, in baseline order, for the
    /// lines beside an image at the head of the page.
    fn starts_beside(page: &Page, foot: f32) -> Vec<f32> {
        content_lines(page)
            .iter()
            .filter(|(baseline, _)| *baseline < foot)
            .map(|(_, runs)| runs[0].0)
            .collect()
    }

    /// Acceptance: the prose sets around an image anchored to the
    /// page, on the side the sheet asks for.
    ///
    /// The same image is anchored at either edge of the page area.
    /// `wrap-flow: end` puts the prose beside it at the end of the
    /// line, and `wrap-flow: start` at the start. Either way the
    /// lines below it run the full measure.
    #[test]
    fn prose_sets_around_an_anchored_image_on_the_side_the_sheet_asks_for() {
        let (left, measure) = {
            let geometry = master(Situation::First(Side::Recto)).geometry;
            (geometry.content_origin().0, geometry.measure())
        };
        let blocks = || std::iter::once(image()).chain(long_prose(8)).collect();

        let beside = with_image(
            "img { position: absolute; top: 0; left: 0; margin-right: 12pt; \
             wrap-flow: end }",
            vec![section(blocks())],
        );
        let page = &beside.pages[0];
        assert_eq!(painted(page), vec![(left, 54.0, IMAGE, IMAGE)]);
        let lines = content_lines(page);
        let leading = lines[1].0 - lines[0].0;
        let beside = |runs: &[Run<'_>]| (runs[0].0 - (left + IMAGE + 12.0)).abs() < 1e-3;
        let narrowed = lines.iter().take_while(|(_, runs)| beside(runs)).count();
        assert!(narrowed > 0, "no line was set beside the image");
        assert!(narrowed < lines.len(), "every line was");
        for (baseline, runs) in &lines[narrowed..] {
            assert!(
                !beside(runs),
                "the line at {baseline} is set beside the image under it",
            );
        }
        // A line is set beside the image when its own band meets it,
        // which the baseline a leading above stands for.
        assert!(lines[narrowed - 1].0 - leading < 54.0 + IMAGE);
        assert!(lines[narrowed].0 - leading >= 54.0 + IMAGE);

        let before = with_image(
            "img { position: absolute; top: 0; right: 0; margin-left: 12pt; \
             wrap-flow: start }",
            vec![section(blocks())],
        );
        let page = &before.pages[0];
        assert_eq!(
            painted(page),
            vec![(left + measure - IMAGE, 54.0, IMAGE, IMAGE)],
        );
        for (baseline, _) in content_lines(page)
            .iter()
            .filter(|(baseline, _)| *baseline < 54.0 + IMAGE)
        {
            let edge = right_edge(page, *baseline);
            assert!(
                edge <= left + measure - IMAGE - 12.0 + 1e-3,
                "the line at {baseline} reaches {edge}, into the image",
            );
        }
    }

    /// Acceptance: prose sets to a polygon written in the sheet, and
    /// nothing is decoded to do it. A polygon reaches layout from the
    /// cascade, so this run has no traced contour at all.
    #[test]
    fn prose_sets_to_a_polygon_written_in_the_sheet() {
        // A right triangle down the leading edge: nothing at the top,
        // the whole box at the foot.
        let wedge = with_image(
            "img { position: absolute; top: 0; left: 0; wrap-flow: end; \
             shape-outside: polygon(0 0, 100% 100%, 0 100%) }",
            vec![section(
                std::iter::once(image()).chain(long_prose(6)).collect(),
            )],
        );
        let page = &wedge.pages[0];
        let left = master(Situation::First(Side::Recto))
            .geometry
            .content_origin()
            .0;
        let starts = starts_beside(page, 54.0 + IMAGE);
        assert!(starts.len() > 3, "not enough lines beside the image");
        assert!(
            starts[0] < left + IMAGE / 4.0,
            "the first line did not reach into the corner the polygon leaves: {}",
            starts[0] - left,
        );
        for pair in starts.windows(2) {
            assert!(
                pair[1] >= pair[0] - 1e-3,
                "the prose did not follow the polygon down: {pair:?}",
            );
        }
        assert!(
            starts.last().expect("a last line") > &(left + IMAGE / 2.0),
            "the polygon never pushed the prose past its middle",
        );

        // The same image with no contour holds every line off its
        // whole width.
        let box_ = with_image(
            "img { position: absolute; top: 0; left: 0; wrap-flow: end }",
            vec![section(
                std::iter::once(image()).chain(long_prose(6)).collect(),
            )],
        );
        for start in starts_beside(&box_.pages[0], 54.0 + IMAGE) {
            assert!(
                start >= left + IMAGE - 1e-3,
                "a line set over the box: {start}",
            );
        }
    }

    /// Acceptance: prose sets to a contour traced from the image's own
    /// alpha under `shape-outside: auto`.
    #[test]
    fn prose_sets_to_a_traced_contour() {
        let pages = traced(
            "img { position: absolute; top: 0; left: 0; wrap-flow: end; \
             shape-outside: auto }",
            |x, y| x <= y,
        );
        let left = master(Situation::First(Side::Recto))
            .geometry
            .content_origin()
            .0;
        let starts = starts_beside(&pages[0], 54.0 + IMAGE);
        assert!(starts.len() > 3, "not enough lines beside the image");
        assert!(
            starts[0] < left + IMAGE / 4.0,
            "the first line did not reach into the clear corner: {}",
            starts[0] - left,
        );
        for pair in starts.windows(2) {
            assert!(
                pair[1] >= pair[0] - 1e-3,
                "the prose did not follow the contour down: {pair:?}",
            );
        }
        assert!(
            starts.last().expect("a last line") > &(left + IMAGE / 2.0),
            "the contour never pushed the prose past the image's middle",
        );
    }

    /// Acceptance: `shape-margin` holds the prose off the contour by
    /// the distance it asks for.
    ///
    /// The contour is the left half of the box, so its edge is
    /// upright and the distance the prose moves is the margin itself.
    /// A sloping edge is held off by at least the margin, because the
    /// contour is read over the band grown by it.
    #[test]
    fn shape_margin_holds_prose_off_the_contour() {
        let sheet = |margin: &str| {
            format!(
                "img {{ position: absolute; top: 0; left: 0; wrap-flow: end; \
                 shape-outside: polygon(0 0, 50% 0, 50% 100%, 0 100%); \
                 shape-margin: {margin} }}"
            )
        };
        let blocks = || {
            vec![section(
                std::iter::once(image()).chain(long_prose(6)).collect(),
            )]
        };
        let close = with_image(&sheet("0"), blocks());
        let off = with_image(&sheet("18pt"), blocks());
        let left = master(Situation::First(Side::Recto))
            .geometry
            .content_origin()
            .0;
        let (close, off) = (
            starts_beside(&close.pages[0], 54.0 + IMAGE),
            starts_beside(&off.pages[0], 54.0 + IMAGE),
        );
        assert!(!close.is_empty() && close.len() == off.len());
        for (near, far) in close.iter().zip(&off) {
            assert!(
                (far - near - 18.0).abs() < 1e-3,
                "the shape margin held the prose off by {}, not 18pt",
                far - near,
            );
            assert!(
                (near - IMAGE / 2.0 - left).abs() < 1e-3,
                "the prose without a margin did not sit on the contour: {near}",
            );
        }
    }

    /// A contour is read band by band, so an image whose alpha leaves
    /// clear space across the middle of it lets the prose set the
    /// full measure there.
    #[test]
    fn prose_sets_through_a_gap_in_a_contour() {
        let pages = traced(
            "img { position: absolute; top: 0; left: 0; wrap-flow: end; \
             shape-outside: auto }",
            // Two bars, with a third of the image clear between them.
            |_, y| !(64..128).contains(&y),
        );
        let left = master(Situation::First(Side::Recto))
            .geometry
            .content_origin()
            .0;
        let through: Vec<f32> = starts_beside(&pages[0], 54.0 + IMAGE)
            .into_iter()
            .filter(|start| (start - left).abs() < 1e-3)
            .collect();
        assert!(
            !through.is_empty(),
            "no line set through the clear space across the image",
        );
    }

    /// Acceptance: an image anchored above a paragraph lands on the
    /// page that paragraph flows onto, not the page the anchor was
    /// written on.
    #[test]
    fn an_anchored_image_lands_on_the_page_the_paragraph_after_it_flows_onto() {
        let css = "img { position: absolute; top: 0; left: 0; wrap-flow: auto }";
        // Which paragraph opens the second page, with nothing
        // anchored: the image goes above that one.
        let bare = with_image(css, vec![section(tagged_prose(30))]);
        assert!(bare.pages.len() > 1, "one page proves nothing here");
        let above: Vec<String> = tagged_lines(&bare.pages[0]);
        let opening = tagged_lines(&bare.pages[1])
            .into_iter()
            .find(|tag| !above.contains(tag))
            .expect("a paragraph opens on the second page");
        let nth: usize = opening
            .trim_start_matches('p')
            .parse()
            .expect("the tag counts the paragraph");

        let mut blocks = tagged_prose(30);
        blocks.insert(nth, image());
        let output = with_image(css, vec![section(blocks)]);
        assert!(painted(&output.pages[0]).is_empty(), "the image waited");
        assert_eq!(painted(&output.pages[1]).len(), 1, "for the page it opens");
    }

    /// Acceptance: an image that asks for no wrapping is positioned
    /// and painted, and the prose under it breaks as if the image is
    /// not there.
    #[test]
    fn an_image_that_asks_for_no_wrapping_leaves_the_prose_where_it_was() {
        let blocks = || std::iter::once(image()).chain(long_prose(6)).collect();
        let bare = with_image("img { position: absolute; top: 0; left: 0 }", vec![]);
        assert!(bare.pages.is_empty());

        let over = with_image(
            "img { position: absolute; top: 0; left: 0 }",
            vec![section(blocks())],
        );
        let without = paginate_styled("img { display: none }", vec![section(long_prose(6))]);
        let page = &over.pages[0];
        let geometry = master(Situation::First(Side::Recto)).geometry;
        let (left, top) = geometry.content_origin();
        assert_eq!(painted(page), vec![(left, top, IMAGE, IMAGE)]);
        assert_eq!(
            content_items(page),
            content_items(&without[0]),
            "the prose broke around an image that excludes nothing",
        );
    }

    /// An image reaches the prose of a quotation as it reaches any
    /// other prose: the bands a quotation is set in are its own, and
    /// the image is the page's.
    #[test]
    fn an_anchored_image_narrows_a_quotation_from_its_own_edge() {
        let quote = Block::Blockquote {
            id: NodeId::UNASSIGNED,
            blocks: long_prose(3),
            attributes: Attributes::default(),
            position: None,
            span: None,
        };
        let output = with_image(
            "img { position: absolute; top: 0; left: 0; margin-right: 12pt; wrap-flow: end } \
             blockquote { margin-left: 36pt }",
            vec![section(vec![image(), quote])],
        );
        let page = &output.pages[0];
        let left = master(Situation::First(Side::Recto))
            .geometry
            .content_origin()
            .0;
        for (baseline, runs) in content_lines(page)
            .iter()
            .filter(|(baseline, _)| *baseline < 54.0 + IMAGE)
        {
            assert!(
                runs[0].0 >= left + IMAGE + 12.0 - 1e-3,
                "the quoted line at {baseline} starts at {}, over the image",
                runs[0].0,
            );
        }
    }

    /// An initial letter goes where the bands it is sunk over are
    /// clear. An image that reaches those bands moves the letter with
    /// them rather than leaves it behind on the image.
    #[test]
    fn an_initial_letter_moves_to_the_bands_an_anchored_image_leaves() {
        let css = "img { position: absolute; top: 0; left: 0; margin-right: 12pt; \
                   wrap-flow: end } p::first-letter { initial-letter: 3 }";
        let output = with_image(
            css,
            vec![section(vec![image(), paragraph(&"prose ".repeat(60))])],
        );
        let page = &output.pages[0];
        let left = master(Situation::First(Side::Recto))
            .geometry
            .content_origin()
            .0;
        let beside = left + IMAGE + 12.0;
        let lines = content_lines(page);
        let (baseline, runs) = lines.first().expect("the paragraph is set");
        assert!(
            *baseline < 54.0 + IMAGE,
            "the paragraph opens at {baseline}, under the image",
        );
        // The letter is the one item larger than the prose, and it
        // stands at the head of the bands the image left.
        let letter = content_items(page)
            .into_iter()
            .find(|(_, _, size, _)| *size > body_size())
            .expect("the initial letter is drawn");
        assert!(
            (letter.0 - beside).abs() < 1e-3,
            "the initial letter is at {} rather than {beside}",
            letter.0,
        );
        assert!(
            runs[0].0 > beside,
            "the first line does not open beside the letter",
        );
    }

    /// Where the bands a letter is sunk over have no room for it, the
    /// paragraph starts under the image instead.
    #[test]
    fn an_initial_letter_with_no_room_beside_it_starts_under_the_image() {
        let geometry = master(Situation::First(Side::Recto)).geometry;
        let (left, measure) = (geometry.content_origin().0, geometry.measure());
        // A gutter wide enough that what is left of the measure holds
        // the letter but not a line beside it.
        let gutter = measure - IMAGE - 30.0;
        let css = format!(
            "img {{ position: absolute; top: 0; left: 0; margin-right: {gutter}pt; \
             wrap-flow: end }} p::first-letter {{ initial-letter: 3 }}"
        );
        let output = with_image(
            &css,
            vec![section(vec![image(), paragraph(&"prose ".repeat(60))])],
        );
        let page = &output.pages[0];
        let lines = content_lines(page);
        let (baseline, _) = lines.first().expect("the paragraph is set");
        assert!(
            *baseline > 54.0 + IMAGE,
            "the paragraph opens at {baseline}, beside an image with no room for the letter",
        );
        let letter = content_items(page)
            .into_iter()
            .find(|(_, _, size, _)| *size > body_size())
            .expect("the initial letter is drawn");
        assert!(
            (letter.0 - left).abs() < 1e-3,
            "the initial letter is at {} rather than {left}",
            letter.0,
        );
    }

    /// The two ways through the pipeline agree over a book with an
    /// image on it as well: the settle the flow runs first sees the
    /// same pages whether the sections were built one at a time or
    /// all at once.
    #[test]
    fn the_stages_compose_over_an_illustrated_book_too() {
        struct Png;
        impl crate::images::ImageLoader for Png {
            fn load(&self, url: &str) -> Option<Vec<u8>> {
                (url == "image.png").then(|| png(192, 192))
            }
        }
        let book = book_of(vec![
            section([vec![image()], long_prose(20)].concat()),
            section([vec![heading("Two"), image()], long_prose(16)].concat()),
        ]);
        let styles = styled(
            "img { position: absolute; top: 0; left: 0; margin-right: 12pt; wrap-flow: end }",
            &book,
        );
        let assets = crate::images::Assets::probe(&book, &styles, &Png);
        let paginator = Paginator::with_assets(registry(), &styles, &assets);

        let staged: Vec<Vec<Fragment>> = book
            .sections
            .iter()
            .map(|section| paginator.section_fragments(section))
            .collect();
        let by_stage = paginator.flow(&book, &staged);
        let in_one = paginator.paginate(&book);

        assert!(in_one.len() > 2, "a book worth splitting");
        assert!(paginator.rebreaks() > 0, "no paragraph met an image");
        assert_eq!(by_stage.len(), in_one.len());
        for (staged, whole) in by_stage.iter().zip(&in_one) {
            assert_eq!(format!("{:?}", staged.items), format!("{:?}", whole.items));
        }
    }

    /// Acceptance: the paragraph beside an image is broken by total
    /// fit, not filled band by band.
    ///
    /// The greedy break of the same text against the same bands packs
    /// every line as far as it goes. The break the flow chose does
    /// not, and its lines sit closer to the bands they were set in:
    /// the slack a break leaves is what its demerits are read from.
    #[test]
    fn the_wrapped_paragraph_is_broken_by_total_fit() {
        let text = "my father had a small estate in nottinghamshire and i was the third \
                    of five sons he sent me to emanuel college in cambridge at fourteen \
                    years old where i resided three years and applied myself close to my \
                    studies but the charge of maintaining me was too great for a narrow \
                    fortune";
        let css = "img { position: absolute; top: 0; left: 0; margin-right: 12pt; \
                   wrap-flow: end } p { text-indent: 0 }";
        let book = book_of(vec![section(vec![image(), paragraph(text)])]);
        let styles = styled(css, &book);
        let output = with_image(css, vec![section(vec![image(), paragraph(text)])]);
        let page = &output.pages[0];

        let geometry = master(Situation::First(Side::Recto)).geometry;
        let measure = geometry.measure();
        let narrow = measure - IMAGE - 12.0;
        let lines = content_lines(page);
        let bands: Vec<f32> = lines
            .iter()
            .map(|(baseline, _)| {
                if *baseline < 54.0 + IMAGE {
                    narrow
                } else {
                    measure
                }
            })
            .collect();
        let set: Vec<String> = lines
            .iter()
            .map(|(_, runs)| runs.iter().map(|run| run.2).collect::<String>())
            .collect();
        assert!(set.len() > 4, "too few lines to disagree over: {set:?}");

        let paginator = Paginator::new(registry(), &styles);
        let style = styles.root().paragraph();
        let width = |text: &str| {
            paginator
                .line_of(text, style)
                .map(|line| paginator.line_width(&line))
                .unwrap_or_default()
        };
        // What filling each band as far as it goes comes to.
        let mut greedy: Vec<String> = Vec::new();
        let mut band = 0;
        for word in text.split_whitespace() {
            let room = bands.get(band).copied().unwrap_or(measure);
            match greedy.last_mut() {
                Some(line) if width(&format!("{line} {word}")) <= room => {
                    line.push(' ');
                    line.push_str(word);
                }
                _ => {
                    greedy.push(word.to_string());
                    band = greedy.len() - 1;
                }
            }
        }
        let chose: Vec<&str> = set.iter().map(|line| line.trim()).collect();
        let packed: Vec<&str> = greedy.iter().map(String::as_str).collect();
        assert_ne!(chose, packed, "the two breaks agree, so nothing is proved");

        // The last line of a break fills what it fills, so the slack
        // under it is not a fault either break is charged for.
        let slack = |broken: &[String]| -> f64 {
            broken
                .iter()
                .take(broken.len() - 1)
                .enumerate()
                .map(|(index, line)| {
                    let room = bands.get(index).copied().unwrap_or(measure);
                    let gap = (room - width(line.trim())) as f64;
                    gap * gap
                })
                .sum()
        };
        let (chosen, filled) = (slack(&set), slack(&greedy));
        assert!(
            chosen < filled,
            "the break the flow chose leaves {chosen} of slack against the greedy {filled}",
        );
    }

    /// A page box divided in two, with a gutter wide enough that a
    /// rectangle over it reaches both columns and covers neither.
    ///
    /// The columns come out 144pt wide, which is the anchored image's
    /// own width, so an image at the leading edge covers the first
    /// column exactly.
    const TWO_COLUMNS: &str = "@page { column-count: 2; column-gap: 48pt }";

    /// The page box `TWO_COLUMNS` resolves to on the page the fixture
    /// sections open on.
    fn two_columns() -> crate::style::PageGeometry {
        styled_geometry(TWO_COLUMNS, Situation::First(Side::Recto))
    }

    /// Every content run of one page as `(column, baseline, start,
    /// end)`, in paint order. The folio is furniture and is not one.
    fn column_runs(page: &Page, geometry: crate::style::PageGeometry) -> Vec<(u32, f32, f32, f32)> {
        page.items
            .iter()
            .filter_map(|item| {
                let DrawItem::Text {
                    x,
                    y,
                    font_id,
                    size,
                    glyphs,
                    ..
                } = item
                else {
                    return None;
                };
                if *size == folio_size() {
                    return None;
                }
                let last = glyphs.last()?;
                let upem = registry().metrics(*font_id)?.units_per_em as f32;
                let advance = registry().advance_width(*font_id, last.id)? as f32;
                let column = (0..geometry.column_count())
                    .rev()
                    .find(|column| *x >= geometry.column_origin(*column).0 - 1e-3)
                    .unwrap_or(0);
                Some((column, *y, *x, last.x + advance / upem * size))
            })
            .collect()
    }

    /// The baselines one column of a page carries, in the order the
    /// flow filled them.
    fn baselines(page: &Page, geometry: crate::style::PageGeometry, column: u32) -> Vec<f32> {
        let mut found: Vec<f32> = Vec::new();
        for (at, baseline, ..) in column_runs(page, geometry) {
            if at == column && found.last() != Some(&baseline) {
                found.push(baseline);
            }
        }
        found
    }

    /// The book the composition tests lay out: an image anchored above
    /// prose enough to fill both columns of a page and run on.
    fn wrapped(css: &str) -> LayoutOutput {
        with_image(
            css,
            vec![section(
                std::iter::once(image()).chain(long_prose(14)).collect(),
            )],
        )
    }

    /// Acceptance: one rectangle over the gutter narrows the column on
    /// either side of it, each from its own edge.
    ///
    /// The image is page geometry and a column is a region of the
    /// page, so neither column owns it. The first column gives up its
    /// trailing edge and the second gives up its leading edge, out of
    /// the one rectangle.
    #[test]
    fn an_image_over_the_gutter_narrows_the_column_on_either_side_of_it() {
        let css = format!(
            "{TWO_COLUMNS} img {{ position: absolute; top: 0; left: 120pt; wrap-flow: both }}"
        );
        let geometry = two_columns();
        let (left, top) = geometry.content_origin();
        let measure = geometry.measure();
        let (near, far) = (left + 120.0, left + 120.0 + IMAGE);
        let foot = top + IMAGE;
        let page = &wrapped(&css).pages[0];
        assert_eq!(painted(page), vec![(near, top, IMAGE, IMAGE)]);
        // One rectangle over the gutter: it reaches into both columns
        // and covers the whole of neither.
        assert!(near > geometry.column_origin(0).0 && near < geometry.column_origin(0).0 + measure);
        assert!(far > geometry.column_origin(1).0 && far < geometry.column_origin(1).0 + measure);

        let mut beside = [0, 0];
        let mut clear = [0, 0];
        for (column, baseline, start, end) in column_runs(page, geometry) {
            let origin = geometry.column_origin(column).0;
            if baseline <= foot {
                beside[column as usize] += 1;
                match column {
                    0 => assert!(
                        end <= near + 1e-3,
                        "a line at {baseline} reaches {end}, over the image"
                    ),
                    _ => assert!(
                        start >= far - 1e-3,
                        "a line at {baseline} starts at {start}, over the image"
                    ),
                }
                continue;
            }
            clear[column as usize] += 1;
            assert!(
                start >= origin - 1e-3 && end <= origin + measure + 1e-3,
                "a line at {baseline} runs {start}..{end}, outside column {column}",
            );
        }
        assert!(
            beside[0] > 0 && beside[1] > 0,
            "no column set beside the image: {beside:?}"
        );
        assert!(
            clear[0] > 0 && clear[1] > 0,
            "no column reached past the image: {clear:?}"
        );

        // Under the image each column has its whole measure back.
        let reach = |column: u32| {
            column_runs(page, geometry)
                .into_iter()
                .filter(|(at, baseline, ..)| *at == column && *baseline > foot)
                .fold((f32::MAX, f32::MIN), |(from, to), (_, _, start, end)| {
                    (from.min(start), to.max(end))
                })
        };
        assert!(
            reach(0).1 > near,
            "the first column never reached past the image"
        );
        assert!(
            reach(1).0 < far - 1e-3,
            "the second column never reached back to its own edge"
        );
    }

    /// Acceptance: an image over the whole width of one column leaves
    /// the other one alone.
    ///
    /// A band the image covers the whole of holds nothing, so the
    /// flow carries the prose of that column to the first band under
    /// it. The other column is set as if the image is not on the page.
    #[test]
    fn an_image_over_a_whole_column_leaves_the_other_alone() {
        let css =
            format!("{TWO_COLUMNS} img {{ position: absolute; top: 0; left: 0; wrap-flow: both }}");
        let geometry = two_columns();
        let (left, top) = geometry.content_origin();
        let measure = geometry.measure();
        assert_eq!(measure, IMAGE, "the image covers the first column exactly");
        let page = &wrapped(&css).pages[0];
        assert_eq!(painted(page), vec![(left, top, IMAGE, IMAGE)]);

        // The same page with the image excluding nothing, which is
        // what an untouched column is read against.
        let plain =
            format!("{TWO_COLUMNS} img {{ position: absolute; top: 0; left: 0; wrap-flow: auto }}");
        let untouched = &wrapped(&plain).pages[0];
        assert_eq!(
            baselines(page, geometry, 1),
            baselines(untouched, geometry, 0),
            "the second column is not set the way an untouched column is",
        );
        for (column, baseline, start, end) in column_runs(page, geometry) {
            if column != 1 {
                continue;
            }
            let origin = geometry.column_origin(1).0;
            assert!(
                start >= origin - 1e-3 && end <= origin + measure + 1e-3,
                "a line at {baseline} runs {start}..{end}, outside the second column",
            );
        }

        // Nothing is set in the bands the image covers, and the first
        // column opens on the first band under it.
        let first = baselines(page, geometry, 0);
        assert!(!first.is_empty(), "the first column set nothing at all");
        assert!(
            first[0] > top + IMAGE,
            "a line at {} is set over the image",
            first[0],
        );
        let leading = first[1] - first[0];
        assert!(
            first[0] - leading <= top + IMAGE,
            "the first column opened {} below the image",
            first[0] - leading - top - IMAGE,
        );
    }

    /// One column's exclusion is not the other's: an image in the
    /// second column does not carry the first column's prose past its
    /// own image.
    #[test]
    fn an_image_in_one_column_is_no_exclusion_in_the_other() {
        let css = format!(
            "{TWO_COLUMNS} \
             .near {{ position: absolute; top: 0; left: 0; wrap-flow: both }} \
             .far {{ position: absolute; top: 0; left: 192pt; wrap-flow: both }}"
        );
        let geometry = two_columns();
        let (left, top) = geometry.content_origin();
        let blocks = || {
            vec![
                image_of("near.png", vec!["near".into()]),
                image_of("far.png", vec!["far".into()]),
            ]
            .into_iter()
            .chain(long_prose(14))
            .collect()
        };
        let output = with_images(
            &css,
            vec![section(blocks())],
            vec![("near.png", png(192, 192)), ("far.png", png(192, 384))],
        );
        let page = &output.pages[0];
        let tall = IMAGE * 2.0;
        assert_eq!(
            painted(page),
            vec![(left, top, IMAGE, IMAGE), (left + 192.0, top, IMAGE, tall),],
        );

        // The first column resumes under its own image rather than
        // under the taller one beside it.
        let first = baselines(page, geometry, 0);
        assert!(
            first[0] > top + IMAGE,
            "a line at {} is set over the near image",
            first[0]
        );
        assert!(
            first[0] < top + tall,
            "the first column waited on the far column's image: {}",
            first[0],
        );
        let second = baselines(page, geometry, 1);
        assert!(
            second[0] > top + tall,
            "a line at {} is set over the far image",
            second[0],
        );
    }

    /// Acceptance: reading order holds. A wrapped column reads to its
    /// foot before the next one begins, and no page turns back.
    #[test]
    fn a_wrapped_column_reads_to_its_foot_before_the_next_begins() {
        let css = format!(
            "{TWO_COLUMNS} img {{ position: absolute; top: 0; left: 120pt; wrap-flow: both }}"
        );
        let output = with_image(
            &css,
            vec![section(
                std::iter::once(image()).chain(tagged_prose(24)).collect(),
            )],
        );
        let mut order: Vec<String> = Vec::new();
        for page in &output.pages {
            let geometry = page_geometry(&css, page);
            let runs = column_runs(page, geometry);
            let columns: Vec<u32> = runs.iter().map(|(column, ..)| *column).collect();
            assert!(
                columns.windows(2).all(|pair| pair[0] <= pair[1]),
                "page {}: the flow went back to a column it had left",
                page.number,
            );
            for column in 0..geometry.column_count() {
                let found = baselines(page, geometry, column);
                assert!(
                    found.windows(2).all(|pair| pair[1] > pair[0]),
                    "page {}: column {column} does not read down the page",
                    page.number,
                );
            }
            for (_, baseline, start, _) in runs {
                let tag = content_lines(page)
                    .iter()
                    .find(|(at, runs)| (at - baseline).abs() < 1e-3 && runs[0].0 == start)
                    .and_then(|(_, runs)| runs[0].2.split_whitespace().next())
                    .unwrap_or_default()
                    .to_string();
                if order.last() != Some(&tag) {
                    order.push(tag);
                }
            }
        }
        let mut seen = order.clone();
        seen.dedup();
        assert_eq!(order, seen, "a paragraph is read in two places: {order:?}");
        assert!(
            order.len() > 8,
            "too few paragraphs to prove an order: {order:?}"
        );
    }

    /// A line that finds no room above a block at the foot of the first
    /// column opens the second column at its head, not under the block.
    #[test]
    fn a_line_moved_past_a_block_at_the_foot_opens_the_next_column_at_its_head() {
        let css = format!("{TWO_COLUMNS} {AT_THE_FOOT} p {{ text-indent: 0 }}");
        let geometry = two_columns();
        for count in 1..40 {
            let pages = paginate_styled(
                &css,
                vec![section(
                    std::iter::once(lifted_quote())
                        .chain(
                            std::iter::repeat_with(|| paragraph("my father had a small estate"))
                                .take(count),
                        )
                        .collect(),
                )],
            );
            let boxes = rects(&pages[0]);
            let [(_, y, _, _, _)] = boxes.as_slice() else {
                panic!("the block paints one box: {boxes:?}");
            };
            assert_eq!(
                prose_below(&pages[0], *y),
                Vec::<f32>::new(),
                "{count} paragraphs: lines set under the block",
            );
            let second = baselines(&pages[0], geometry, 1);
            let leading = 2.0 * body_size();
            assert!(
                second
                    .first()
                    .is_none_or(|first| *first < geometry.content_origin().1 + leading),
                "{count} paragraphs: the second column opens at {second:?}",
            );
        }
    }

    /// Acceptance: nothing crosses a gutter, the exclusion included.
    /// Every line of every page sits inside the column it was set in,
    /// and no line is set in the space between two columns.
    #[test]
    fn nothing_crosses_a_gutter_beside_an_image() {
        let css = format!(
            "{TWO_COLUMNS} img {{ position: absolute; top: 0; left: 120pt; wrap-flow: both }}"
        );
        let output = wrapped(&css);
        assert!(
            output.pages.len() > 1,
            "one page proves nothing about a book"
        );
        for page in &output.pages {
            let geometry = page_geometry(&css, page);
            let measure = geometry.measure();
            let gutter = geometry.column_origin(1).0 - geometry.columns.gap;
            for (column, baseline, start, end) in column_runs(page, geometry) {
                let origin = geometry.column_origin(column).0;
                assert!(
                    start >= origin - 1e-3 && end <= origin + measure + 1e-3,
                    "page {}: a line at {baseline} runs {start}..{end}, outside column {column}",
                    page.number,
                );
                assert!(
                    end <= gutter + 1e-3 || start >= gutter + geometry.columns.gap - 1e-3,
                    "page {}: a line at {baseline} runs {start}..{end}, into the gutter",
                    page.number,
                );
            }
        }
    }
}
