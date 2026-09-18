//! Navigation: the links on each page, with the area each covers and
//! where it goes, and the outline of the book's headings.
//!
//! Both read the destination table the flow built. A link's area is
//! read off the runs its text was set in, one area for each line, so a
//! link broken across two lines is followed from either line and not
//! from the space between them.

use std::collections::BTreeMap;

use crate::Warning;
use crate::content::{
    Anchors, Block, Book, Inline, LinkTarget, NodeId, SourcePos, cell_blocks, inline_nodes,
    item_blocks, origin, text,
};
use crate::fonts::FontRegistry;
use crate::pages::{DrawItem, Link, LinkTo, Navigation, OutlineEntry, Page, PageBox};

/// One link as the book writes it.
struct Written<'b> {
    /// The ids of the link and everything inside it.
    nodes: std::ops::Range<u32>,
    url: &'b str,
    source: Option<&'b str>,
    position: Option<SourcePos>,
}

/// One heading as the book writes it.
struct Heading {
    node: NodeId,
    level: u8,
    title: String,
}

/// Puts the links of one laid-out book on its pages, and returns the
/// outline of its headings and what the links had to complain about:
/// a link whose url names nothing in the book is set as text, and says
/// so.
pub(crate) fn navigation(
    book: &Book,
    pages: &mut [Page],
    targets: &BTreeMap<NodeId, PageBox>,
    registry: &FontRegistry,
) -> (Navigation, Vec<Warning>) {
    let mut written = Vec::new();
    let mut headings = Vec::new();
    for section in &book.sections {
        for block in &section.blocks {
            if let Block::Heading {
                id, level, inlines, ..
            } = block
            {
                headings.push(Heading {
                    node: *id,
                    level: u8::from(*level),
                    title: title(inlines),
                });
            }
        }
        links_in_blocks(&section.blocks, section.source.as_deref(), &mut written);
    }
    let mut warnings = Vec::new();
    if !written.is_empty() {
        let anchors = book.anchors();
        let places: Vec<Option<LinkTo>> = written
            .iter()
            .map(|link| {
                let to = place(&anchors, targets, link);
                if let Err(unplaced) = &to {
                    let message = match unplaced {
                        Some(node) => format!(
                            "`{}` names an element on no page. The text is not a link.",
                            anchors.name(*node).unwrap_or(link.url)
                        ),
                        None => format!(
                            "`{}` names nothing in the book. The text is not a link.",
                            link.url
                        ),
                    };
                    let at = origin(link.source, link.position);
                    if !warnings
                        .iter()
                        .any(|seen: &Warning| seen.message == message)
                    {
                        warnings.push(Warning {
                            message,
                            origin: (!at.is_empty()).then_some(at),
                        });
                    }
                }
                to.ok()
            })
            .collect();
        place_links(pages, &written, &places, registry);
    }
    let outline = outline(&headings, targets);
    (Navigation { outline }, warnings)
}

/// Where one link goes, or why it goes nowhere: the element it names
/// is on no page, or it names nothing in the book.
fn place(
    anchors: &Anchors,
    targets: &BTreeMap<NodeId, PageBox>,
    link: &Written<'_>,
) -> Result<LinkTo, Option<NodeId>> {
    match anchors.resolve(link.url, link.source) {
        LinkTarget::Outside => Ok(LinkTo::Uri(link.url.to_string())),
        LinkTarget::Node(node) => targets
            .get(&node)
            .map(|place| LinkTo::Place {
                node,
                place: *place,
            })
            .ok_or(Some(node)),
        LinkTarget::Missing => Err(None),
    }
}

/// The words of a heading as an outline shows them, on one line.
fn title(inlines: &[Inline]) -> String {
    text(inlines)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn links_in_blocks<'b>(blocks: &'b [Block], source: Option<&'b str>, out: &mut Vec<Written<'b>>) {
    for block in blocks {
        match block {
            Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
                links_in_inlines(inlines, source, out)
            }
            Block::Blockquote { blocks, .. } => links_in_blocks(blocks, source, out),
            Block::List { items, .. } => {
                for blocks in item_blocks(items) {
                    links_in_blocks(blocks, source, out);
                }
            }
            Block::Table { head, body, .. } => {
                for blocks in cell_blocks(head, body) {
                    links_in_blocks(blocks, source, out);
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

fn links_in_inlines<'b>(
    inlines: &'b [Inline],
    source: Option<&'b str>,
    out: &mut Vec<Written<'b>>,
) {
    for inline in inlines {
        match inline {
            Inline::Link {
                id, url, position, ..
            } => {
                let first = id.get();
                out.push(Written {
                    nodes: first..first + inline_nodes(inline),
                    url,
                    source,
                    position: *position,
                });
            }
            Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Span { children, .. } => links_in_inlines(children, source, out),
            Inline::Text { .. } | Inline::Code { .. } | Inline::Break { .. } => {}
        }
    }
}

/// Puts on each page the links set on it, with one area for each line.
///
/// Runs of one link on one baseline that meet, or nearly meet, are one
/// area. A link that runs to the foot of one column and on at the head
/// of the next is two areas. A link that runs on to the next page is on
/// both pages.
fn place_links(
    pages: &mut [Page],
    written: &[Written<'_>],
    places: &[Option<LinkTo>],
    registry: &FontRegistry,
) {
    for (index, page) in pages.iter_mut().enumerate() {
        // Each area with the link it belongs to, the baseline it is on,
        // and the size of its face, which is how near two runs have to
        // be to meet.
        let mut open: Vec<(usize, f32, f32, PageBox)> = Vec::new();
        for item in &page.items {
            let DrawItem::Text {
                y,
                size,
                origin: Some(origin),
                ..
            } = item
            else {
                continue;
            };
            let node = origin.node.element().get();
            let at = written.partition_point(|link| link.nodes.start <= node);
            let Some(which) = at.checked_sub(1) else {
                continue;
            };
            if !written[which].nodes.contains(&node) || places[which].is_none() {
                continue;
            }
            let Some((_, _, area)) = run_area(registry, index, item) else {
                continue;
            };
            let meets = |other: &PageBox, size: f32| {
                area.x <= other.x + other.width + size && other.x <= area.x + area.width + size
            };
            match open.iter_mut().find(|(link, baseline, near, other)| {
                *link == which && baseline == y && meets(other, near.max(*size))
            }) {
                Some((_, _, near, other)) => {
                    *near = near.max(*size);
                    *other = union(other, &area);
                }
                None => open.push((which, *y, *size, area)),
            }
        }
        let mut links: Vec<(usize, Link)> = Vec::new();
        for (which, _, _, area) in open {
            match links.iter_mut().find(|(link, _)| *link == which) {
                Some((_, link)) => link.areas.push(area),
                None => links.push((
                    which,
                    Link {
                        areas: vec![area],
                        to: places[which]
                            .clone()
                            .expect("only a link that goes somewhere is open"),
                    },
                )),
            }
        }
        page.links = links.into_iter().map(|(_, link)| link).collect();
    }
}

/// The smallest box that holds both.
fn union(one: &PageBox, other: &PageBox) -> PageBox {
    let (left, top) = (one.x.min(other.x), one.y.min(other.y));
    let right = (one.x + one.width).max(other.x + other.width);
    let bottom = (one.y + one.height).max(other.y + other.height);
    PageBox {
        page: one.page,
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

/// The headings, nested by level: each takes the headings after it
/// that are deeper, up to the next one at its level or above. A
/// heading that is on no page is left out.
fn outline(headings: &[Heading], targets: &BTreeMap<NodeId, PageBox>) -> Vec<OutlineEntry> {
    let mut roots: Vec<OutlineEntry> = Vec::new();
    for heading in headings {
        let Some(place) = targets.get(&heading.node) else {
            continue;
        };
        let entry = OutlineEntry {
            title: heading.title.clone(),
            level: heading.level,
            place: *place,
            children: Vec::new(),
        };
        let mut siblings = &mut roots;
        while siblings.last().is_some_and(|last| last.level < entry.level) {
            siblings = &mut siblings.last_mut().expect("checked above").children;
        }
        siblings.push(entry);
    }
    roots
}

/// The node a run of text was written in, the pseudo-element it was
/// cut from, and the area the run covers on the page at `index`: its
/// glyphs across, and its face's ascent and descent down. Nothing for
/// text the engine wrote itself.
pub(crate) fn run_area(
    registry: &FontRegistry,
    index: usize,
    item: &DrawItem,
) -> Option<(NodeId, Option<NodeId>, PageBox)> {
    let DrawItem::Text {
        y,
        font_id,
        size,
        glyphs,
        origin: Some(origin),
        pseudo_element,
        ..
    } = item
    else {
        return None;
    };
    let metrics = registry.metrics(*font_id)?;
    let scale = size / metrics.units_per_em as f32;
    let (first, last) = (glyphs.first()?, glyphs.last()?);
    let advance = registry.advance_width(*font_id, last.id).unwrap_or(0) as f32 * scale;
    let (left, right) = (first.x.min(last.x), first.x.max(last.x + advance));
    let top = y - metrics.ascender as f32 * scale;
    let bottom = y - metrics.descender as f32 * scale;
    Some((
        origin.node,
        *pseudo_element,
        PageBox {
            page: index as u32,
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        },
    ))
}
