//! The images the sheet lifts out of the flow, and the bands the
//! prose around one is set in.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::content::{Block, Book, NodeId, origin};
use crate::lines::{Measure, Span};
use crate::pages::DrawItem;
use crate::style::{ComputedStyle, Edges, Inset, PageGeometry, Position, ShapeOutside, WrapFlow};

use super::Paginator;
use super::build::{Reflow, carry_over, set_lines};
use super::cap::Cap;
use super::flow::Flow;
use super::fragment::Fragment;

impl Paginator<'_> {
    /// The images the sheet anchored to the page, in document order.
    ///
    /// Each is sized as CSS 2.1 sizes a replaced element with no
    /// width or height of its own: its intrinsic size, scaled down
    /// where that does not fit the page area.
    pub(super) fn anchored_images(&self, book: &Book) -> Vec<AnchoredImage> {
        fn walk(
            paginator: &Paginator,
            blocks: &[Block],
            source: Option<&str>,
            anchored: &mut Vec<AnchoredImage>,
        ) {
            for block in blocks {
                match block {
                    Block::Blockquote { blocks, .. } => walk(paginator, blocks, source, anchored),
                    Block::Image {
                        id, url, position, ..
                    } => {
                        let style = paginator.styles.style(*id);
                        if style.position != Position::Absolute {
                            continue;
                        }
                        let origin = origin(source, *position);
                        let Some((asset, intrinsic)) = paginator.assets.lookup(url) else {
                            paginator.missing(url, origin);
                            continue;
                        };
                        let (available, height) =
                            paginator.styles.default_page().geometry.content_size();
                        let margin = style.margin;
                        let (width, height) = fit(
                            intrinsic.size(),
                            (available - margin.inline()).max(0.0),
                            (height - margin.top - margin.bottom).max(0.0),
                        );
                        anchored.push(AnchoredImage {
                            node: *id,
                            asset,
                            width,
                            height,
                            inset: style.inset,
                            margin,
                            wrap: style.wrap_flow,
                            shape: paginator.shape(style, asset, (width, height)),
                        });
                    }
                    _ => {}
                }
            }
        }
        let mut anchored = Vec::new();
        for section in &book.sections {
            walk(
                self,
                &section.blocks,
                section.source.as_deref(),
                &mut anchored,
            );
        }
        anchored
    }

    /// The contour one anchored image's prose keeps clear of, in the
    /// coordinates of the box its insets place.
    ///
    /// `auto` is what the trace stage left, laid over the image
    /// inside its margins. An image the tracer had no alpha for
    /// contributes its box. A polygon is read against the whole box,
    /// margins and all, which is what a percentage in it measures.
    fn shape(&self, style: &ComputedStyle, asset: u32, size: (f32, f32)) -> Option<Shape> {
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
                .get(asset)?
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

/// One image the sheet lifted out of the flow: what it paints, how
/// far its insets put it from the page area, and which side the prose
/// sets on.
#[derive(Debug, Clone)]
pub(super) struct AnchoredImage {
    /// The node it was written at, which is what decides its page.
    node: NodeId,
    /// Index into the asset table.
    asset: u32,
    /// Width in points, after any scaling.
    width: f32,
    /// Height in points, after any scaling.
    height: f32,
    /// What the insets say about where it sits.
    inset: Edges<Inset>,
    /// What it keeps clear of prose around itself.
    margin: Edges,
    /// Which side of it the prose sets on.
    wrap: WrapFlow,
    /// The contour the prose keeps clear of in place of the box,
    /// from `shape-outside`.
    shape: Option<Shape>,
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

impl AnchoredImage {
    /// What it keeps to itself on a page of this geometry: the image
    /// and the margins around it.
    ///
    /// An inset measures from the page area, the box the margins
    /// leave, and a negative inset reaches into the margin. Where
    /// both insets of an axis are lengths, the leading one places the
    /// box. Where neither is a length, the box sits at the edge of
    /// the page area.
    fn rect(&self, geometry: PageGeometry) -> Rect {
        let (left, top) = geometry.content_origin();
        let (width, height) = geometry.content_size();
        let w = self.width + self.margin.inline();
        let h = self.height + self.margin.top + self.margin.bottom;
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
                self.inset.left.points(),
                self.inset.right.points(),
                left,
                width,
                w,
            ),
            y: place(
                self.inset.top.points(),
                self.inset.bottom.points(),
                top,
                height,
                h,
            ),
            w,
            h,
        }
    }

    /// The image itself, inside the margins it keeps.
    pub(super) fn item(&self, geometry: PageGeometry) -> DrawItem {
        let rect = self.rect(geometry);
        DrawItem::Image {
            x: rect.x + self.margin.left,
            y: rect.y + self.margin.top,
            w: self.width,
            h: self.height,
            asset: self.asset,
        }
    }
}

/// The images one book anchors, and the page each one landed on.
///
/// Which page an image falls on comes from the flow. Where it sits on
/// that page comes from the sheet. The flow runs once with nothing in
/// the way to answer the first question, and it then holds the
/// answer. An image narrows the page it was given, and the flow never
/// asks that page again.
#[derive(Debug, Default)]
pub(crate) struct AnchoredImages {
    pub(super) all: Vec<AnchoredImage>,
    /// Which images a page carries, by page index.
    pub(super) by_page: BTreeMap<usize, Vec<usize>>,
}

impl AnchoredImages {
    /// The images of a book, on the pages the anchor pass gave them.
    pub(super) fn on(all: Vec<AnchoredImage>, anchors: &BTreeMap<NodeId, usize>) -> AnchoredImages {
        let mut by_page: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (index, image) in all.iter().enumerate() {
            let Some(page) = anchors.get(&image.node) else {
                continue;
            };
            by_page.entry(*page).or_default().push(index);
        }
        AnchoredImages { all, by_page }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.by_page.is_empty()
    }
}

/// An image's size inside a box that has to hold it: its own, scaled
/// down in proportion where either side does not fit.
fn fit((width, height): (f32, f32), available: f32, room: f32) -> (f32, f32) {
    let scale = |value: f32, from: f32, to: f32| {
        if from > 0.0 { value * to / from } else { value }
    };
    let (mut width, mut height) = (width, height);
    if width > available {
        height = scale(height, width, available);
        width = available;
    }
    if height > room {
        width = scale(width, height, room);
        height = room;
    }
    (width, height)
}

impl Flow<'_, '_> {
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
            // The profile is read against where the line sits, which
            // is where `place` is about to put it.
            let lead = if self.column_empty() {
                0.0
            } else {
                set[at].lead
            };
            let top = self.cursor + lead + set[at].fixed;
            let from = if at == 0 { 0 } else { ends[at - 1] };
            let profile = self.profile(top, reflow, from == 0);
            if profile.is_some() || narrowed {
                narrowed = profile.is_some();
                let profile = profile.unwrap_or_else(|| Profile::plain(reflow, from == 0));
                let paginator = self.paginator;
                paginator.rebreaks.set(paginator.rebreaks.get() + 1);
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
                carry_over(&mut fresh, &set[at..]);
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
            .unwrap_or(0.0);
    }

    /// The images on the page being built, in the coordinates of the
    /// column being filled.
    fn holes(&self) -> Vec<Hole<'_>> {
        let index = self.pages.len();
        let Some(anchored) = self.anchored.by_page.get(&index) else {
            return Vec::new();
        };
        let geometry = self.paginator.master(index, &self.slot).geometry;
        let origin = geometry.column_origin(self.column);
        anchored
            .iter()
            .map(|at| &self.anchored.all[*at])
            .filter(|image| image.wrap != WrapFlow::Auto)
            .map(|image| Hole {
                rect: image.rect(geometry).within(origin),
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
    fn profile(&self, top: f32, reflow: &Reflow, opening: bool) -> Option<Profile> {
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
            let mut free = clear(base, &holes, y, y + leading, narrowest);
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
        Run, body_size, book_of, content_items, content_lines, heading, image, long_prose, master,
        paginate_styled, painted, paragraph, png, registry, right_edge, section, styled,
        tagged_lines, tagged_prose, with_image,
    };
    use crate::pages::{Page, Side};
    use crate::style::Situation;

    /// The image the tests anchor is 144pt square.
    const IMAGE: f32 = 144.0;

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
        let assets = crate::images::Assets::probe(&book, &Alpha(alpha_png(covered)));
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

    /// Acceptance: the anchor map is settled with nothing in the way
    /// and then held. An image that narrows its own page can push the
    /// paragraph it hangs from onto the next page. The image stays
    /// where the settle put it.
    #[test]
    fn the_anchor_map_is_settled_once_and_held() {
        let css = |wrap| {
            format!(
                "img {{ position: absolute; top: 0; left: 0; margin-right: 12pt; \
                 wrap-flow: {wrap} }}"
            )
        };
        let mut blocks = tagged_prose(30);
        // Anchored on a page the image then narrows, so the
        // paragraph under the anchor moves and the image does not.
        blocks.insert(4, image());
        let settled = with_image(&css("auto"), vec![section(blocks.clone())]);
        let wrapped = with_image(&css("end"), vec![section(blocks)]);

        let page_of = |output: &LayoutOutput| {
            output
                .pages
                .iter()
                .position(|page| !painted(page).is_empty())
                .expect("the image is painted")
        };
        assert_eq!(page_of(&settled), page_of(&wrapped));
        assert!(
            tagged_lines(&wrapped.pages[0]).len() < tagged_lines(&settled.pages[0]).len(),
            "the image did not narrow the page it landed on",
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
        let assets = crate::images::Assets::probe(&book, &Png);
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
}
