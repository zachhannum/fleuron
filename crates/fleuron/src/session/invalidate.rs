//! What one run is compared against, so that the next run knows
//! which stages an edit left standing.

use std::hash::{DefaultHasher, Hash, Hasher};

use crate::content::{Block, Book, Metadata};
use crate::lines::Patterns;
use crate::style::{ComputedStyle, Content, Position, StyleTree};

use super::Stale;
use super::key::{hash_edges, hash_geometry, hash_layout, hash_nodes, hash_shape};

/// The page a section's lines were broken against: the measure
/// always, and the content height only for a book with an image in
/// it. An image is the one thing fragment building sizes against the
/// page's own height, so a book without one never reads that height,
/// and a page that grows taller leaves its prose broken where it
/// was.
///
/// A book that anchors an image to the page carries the exclusions as
/// well. The flow resolves their geometry. A section whose paragraphs
/// can be set again beside an image keeps what it takes to set them.
/// A book that gains its first image builds its sections again.
#[derive(Debug, Clone, Copy)]
pub(super) struct Against {
    measure: f32,
    height: Option<f32>,
    exclusions: Option<u64>,
}

impl Against {
    pub(super) fn of(styles: &StyleTree, images: bool) -> Against {
        let geometry = styles.default_page().geometry;
        let mut anchored = DefaultHasher::new();
        let mut any = false;
        for style in styles
            .styles()
            .iter()
            .filter(|style| style.position == Position::Absolute)
        {
            any = true;
            style.position.hash(&mut anchored);
            style.wrap_flow.hash(&mut anchored);
            hash_shape(&style.shape_outside, &mut anchored);
            style.shape_margin.to_bits().hash(&mut anchored);
            for inset in [
                style.inset.top,
                style.inset.right,
                style.inset.bottom,
                style.inset.left,
            ] {
                inset.points().map(f32::to_bits).hash(&mut anchored);
            }
            hash_edges(style.margin, &mut anchored);
        }
        Against {
            measure: geometry.measure(),
            height: images.then(|| geometry.content_size().1),
            exclusions: any.then(|| anchored.finish()),
        }
    }

    pub(super) fn hash_into(self, h: &mut DefaultHasher) {
        (self.measure.to_bits(), self.height.map(f32::to_bits)).hash(h);
        self.exclusions.hash(h);
    }
}

/// Whether any section of the book places an image.
pub(super) fn has_images(book: &Book) -> bool {
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
pub(super) struct Prints {
    /// Which nodes ask for a contour to be traced, and from what.
    trace: u64,
    /// The measure, what every distinct style says about breaking,
    /// and which style each node resolved to.
    breaks: u64,
    /// Page geometry, and the named page each element asks for.
    flow: u64,
    /// The margin boxes, and what they paint.
    paint: u64,
}

impl Prints {
    pub(super) fn of(styles: &StyleTree, images: bool) -> Prints {
        // Both halves of what the stage keys on: which nodes name a
        // contour, and which of them the flow reads one for. A sheet
        // that anchors an image already asking for `auto` reaches the
        // stage through the second.
        let mut trace = DefaultHasher::new();
        for style in styles.styles() {
            hash_shape(&style.shape_outside, &mut trace);
            style.excludes().hash(&mut trace);
        }
        hash_nodes(styles, &mut trace);

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
            trace: trace.finish(),
            breaks: breaks.finish(),
            flow: flow.finish(),
            paint: paint.finish(),
        }
    }

    /// The deepest stage a move from `self` to `fresh` invalidates.
    pub(super) fn against(&self, fresh: &Prints) -> Stale {
        if self.trace != fresh.trace {
            Stale::Trace
        } else if self.breaks != fresh.breaks {
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
pub(super) fn section_local(styles: &StyleTree) -> bool {
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

/// What the book's declared language decides: the patterns its words
/// break by, and the tag the warning names where there are no
/// patterns for it. Two languages without patterns break the same
/// lines and warn about different tags.
pub(super) fn hyphenation(metadata: &Metadata) -> (Patterns, Option<&str>) {
    let patterns = Patterns::of(metadata);
    let unknown = metadata.language().filter(|_| patterns == Patterns::NONE);
    (patterns, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::testing::{prose, section, sheets, three_chapters};
    use crate::style::CounterStyle;

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
}
