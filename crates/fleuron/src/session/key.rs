//! The fingerprint of one section: everything about it that
//! decides where its lines break.

use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::content::{Block, Inline, NodeId, Section, rows};
use crate::layout::{Named, References};
use crate::lines::Patterns;
use crate::style::{
    Background, BackgroundSize, ColumnRule, Columns, ComputedStyle, Coord, Edges, PageGeometry,
    ShapeOutside, StyleTree, Width,
};

use super::invalidate::Against;

/// What one section's lines were built from: the file it came from,
/// its content, and the style every node in it resolved to. Node ids
/// are deliberately absent, because they renumber globally on every
/// edit and a chapter nothing touched would then miss its own
/// cache. So is the stretch of the source a node was read from,
/// which moves whenever a byte above it does and changes no line.
///
/// A section's references reach past it, to the elements they name.
/// The words of each, and whether an element carries the id at all,
/// are known before anything is laid out, so they are part of it.
pub(super) fn section_key(
    section: &Section,
    styles: &StyleTree,
    against: Against,
    assets: Option<usize>,
    hyphenation: (Patterns, Option<&str>),
    references: &References,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    let h = &mut hasher;
    against.hash_into(h);
    assets.hash(h);
    hyphenation.hash(h);
    (&section.source, &section.title, section.position).hash(h);
    hash_node(section.id, styles, h);
    hash_blocks(&section.blocks, styles, h);
    if styles.refers() {
        let named = Named::in_section(section, styles);
        for id in named.pages.iter().chain(&named.texts) {
            (id, references.text(id)).hash(h);
        }
    }
    hasher.finish()
}

/// What one section's lines were built from on the pass that prints
/// the pages its references name: what they were built from on the
/// pass before, and the folio each of those references prints.
pub(super) fn settled_key(key: u64, pages: &[String], found: &BTreeMap<String, u32>) -> u64 {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    for id in pages {
        (id, found.get(id)).hash(&mut hasher);
    }
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
                attributes: _,
                span: _,
            } => {
                (0u8, level, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(inlines, styles, h);
            }
            Block::Paragraph {
                id,
                inlines,
                position,
                attributes: _,
                span: _,
            } => {
                (1u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(inlines, styles, h);
            }
            Block::Blockquote {
                id,
                blocks,
                position,
                attributes: _,
                span: _,
            } => {
                (2u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_blocks(blocks, styles, h);
            }
            Block::ThematicBreak {
                id,
                position,
                attributes: _,
                span: _,
            } => {
                (3u8, position).hash(h);
                hash_node(*id, styles, h);
            }
            Block::Image {
                id,
                url,
                alt,
                position,
                attributes: _,
                span: _,
            } => {
                (4u8, url, alt, position).hash(h);
                hash_node(*id, styles, h);
            }
            Block::Table {
                id,
                head,
                body,
                position,
                attributes: _,
                span: _,
            } => {
                (5u8, position, head.len()).hash(h);
                hash_node(*id, styles, h);
                for row in rows(head, body) {
                    (row.position, row.cells.len()).hash(h);
                    hash_node(row.id, styles, h);
                    for cell in &row.cells {
                        (cell.position, cell.align).hash(h);
                        hash_node(cell.id, styles, h);
                        hash_blocks(&cell.blocks, styles, h);
                    }
                }
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
                attributes: _,
                span: _,
            } => {
                (0u8, value, position).hash(h);
                hash_node(*id, styles, h);
            }
            Inline::Emphasis {
                id,
                children,
                position,
                attributes: _,
                span: _,
            } => {
                (1u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(children, styles, h);
            }
            Inline::Strong {
                id,
                children,
                position,
                attributes: _,
                span: _,
            } => {
                (2u8, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(children, styles, h);
            }
            Inline::Code {
                id,
                value,
                position,
                attributes: _,
                span: _,
            } => {
                (3u8, value, position).hash(h);
                hash_node(*id, styles, h);
            }
            Inline::Link {
                id,
                url,
                children,
                position,
                attributes: _,
                span: _,
            } => {
                (4u8, url, position).hash(h);
                hash_node(*id, styles, h);
                hash_inlines(children, styles, h);
            }
        }
    }
}

/// One node's resolved styling, and its pseudo-elements beside it.
fn hash_node(id: NodeId, styles: &StyleTree, h: &mut DefaultHasher) {
    hash_layout(styles.style(id), h);
    for pseudo in [
        styles.first_letter(id),
        styles.first_line(id),
        styles.before(id),
        styles.after(id),
    ] {
        match pseudo {
            Some(style) => {
                1u8.hash(h);
                hash_layout(style, h);
            }
            None => 0u8.hash(h),
        }
    }
}

/// Everything a fragment is built from. Destructured field by field
/// on purpose: a property added to `ComputedStyle` stops compiling
/// here until somebody says which stage it belongs to.
pub(super) fn hash_layout(style: &ComputedStyle, h: &mut DefaultHasher) {
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
        // Whether an image is in the flow decides whether the section
        // holds a fragment for it or an anchor. Where the image then
        // sits, and which side the prose sets on, belong to the flow.
        position,
        inset: _,
        wrap_flow: _,
        shape_outside: _,
        shape_margin: _,
        margin,
        padding,
        border,
        background,
        box_decoration_break,
        width,
        border_collapse,
        break_before,
        break_after,
        break_inside,
        // A spanning block breaks to the whole content box.
        column_span,
    } = style;
    border_collapse.hash(h);
    match width {
        Width::Auto => 0u8.hash(h),
        Width::Points(points) => (1u8, points.to_bits()).hash(h),
        Width::Percent(percent) => (2u8, percent.to_bits()).hash(h),
    }
    (font_id, font_size.to_bits(), line_height.to_bits(), color).hash(h);
    (letter_spacing.to_bits(), font_variant_caps, text_transform).hash(h);
    (text_align, text_justify, hanging_punctuation).hash(h);
    (text_indent.to_bits(), hyphens, orphans, widows).hash(h);
    (content, string_set, counter_reset, initial_letter).hash(h);
    position.hash(h);
    (break_before, break_after, break_inside, column_span).hash(h);
    hash_background(background, h);
    box_decoration_break.hash(h);
    hash_edges(*margin, h);
    hash_edges(*padding, h);
    hash_edges(border.widths(), h);
    for edge in [border.top, border.right, border.bottom, border.left] {
        edge.color.hash(h);
    }
}

/// What one box paints behind its content. The tint and the image
/// both reach the display structure through the flow, and neither
/// moves a line, but a section keyed without them would serve a page
/// still painted the way it was.
pub(super) fn hash_background(background: &Background, h: &mut DefaultHasher) {
    let Background {
        color,
        image,
        repeat,
        size,
        position,
    } = background;
    (color, image, repeat).hash(h);
    match size {
        BackgroundSize::Auto => 0u8.hash(h),
        BackgroundSize::Cover => 1u8.hash(h),
        BackgroundSize::Contain => 2u8.hash(h),
        BackgroundSize::Fixed { width, height } => {
            3u8.hash(h);
            for axis in [width, height] {
                hash_coord(*axis, h);
            }
        }
    }
    hash_coord(Some(position.x), h);
    hash_coord(Some(position.y), h);
}

/// One coordinate, or the absence of one: `f32` is not `Hash`, and a
/// percentage and a length of the same number are different answers.
fn hash_coord(coord: Option<Coord>, h: &mut DefaultHasher) {
    match coord {
        None => 0u8.hash(h),
        Some(Coord::Points(points)) => (1u8, points.to_bits()).hash(h),
        Some(Coord::Percent(percent)) => (2u8, percent.to_bits()).hash(h),
    }
}

pub(super) fn hash_geometry(geometry: PageGeometry, h: &mut DefaultHasher) {
    let PageGeometry {
        width,
        height,
        margin,
        columns,
    } = geometry;
    (width.to_bits(), height.to_bits()).hash(h);
    hash_edges(margin, h);
    let Columns {
        count,
        width,
        gap,
        rule,
    } = columns;
    (count, width.map(f32::to_bits), gap.to_bits()).hash(h);
    let ColumnRule { style, width } = rule;
    (style, width.to_bits()).hash(h);
}

/// What one contour is: which of the three `shape-outside` says, and
/// the points where it says a polygon.
pub(super) fn hash_shape(shape: &ShapeOutside, h: &mut DefaultHasher) {
    match shape {
        ShapeOutside::None => 0u8.hash(h),
        ShapeOutside::Auto => 1u8.hash(h),
        ShapeOutside::Polygon(points) => {
            2u8.hash(h);
            for point in points {
                for coord in [point.x, point.y] {
                    match coord {
                        Coord::Points(points) => (0u8, points.to_bits()).hash(h),
                        Coord::Percent(percent) => (1u8, percent.to_bits()).hash(h),
                    }
                }
            }
        }
    }
}

pub(super) fn hash_edges(edges: Edges, h: &mut DefaultHasher) {
    let Edges {
        top,
        right,
        bottom,
        left,
    } = edges;
    [top, right, bottom, left].map(f32::to_bits).hash(h);
}

pub(super) fn hash_nodes(styles: &StyleTree, h: &mut DefaultHasher) {
    for node in styles.nodes() {
        (node.id, node.element, node.style, node.first_letter).hash(h);
        (node.first_line, node.before, node.after).hash(h);
    }
}
