//! A table: the columns sized once from the sheet, each row set whole
//! as one fragment, and the header rows set again at the top of every
//! page the table continues onto.
//!
//! A column takes the width the cell of the first row asks for, and
//! the columns that ask for none share what is left. Nothing inside a
//! cell is measured to decide it, so every line is still broken
//! against a measure it was handed.
//!
//! Collapsed, two cells that meet draw one rule between them. The
//! rule is the widest of the borders that meet there. A row draws the
//! rule under itself, and the first row the rule over it too, so a
//! header row carries the rule under it wherever it is set again.

use crate::content::{Block, NodeId, Row, SourcePos, origin, rows};
use crate::pages::DrawItem;
use crate::style::{Border, BorderCollapse, Color, ComputedStyle, Edges, StyleTree};

use super::build::Builder;
use super::flow::{Painted, shift};
use super::fragment::{BreakPoint, Decoration, Fragment, Marks, Piece, TableRow};

impl<'a> Builder<'a, '_> {
    /// One table, at `x` from the content box's leading edge and
    /// sized against `measure`. Each row is one fragment, so a page
    /// can end between two rows and never inside one.
    pub(super) fn table(
        &mut self,
        id: NodeId,
        head: &[Row],
        body: &[Row],
        position: Option<SourcePos>,
        x: f32,
        measure: f32,
    ) {
        let styles: &'a StyleTree = self.paginator.styles;
        let style = styles.style(id);
        let collapse = style.border_collapse == BorderCollapse::Collapse;
        // Collapsed, the table's own border is drawn on the rules its
        // cells share, and its padding is not used.
        let frame = if collapse {
            ComputedStyle {
                border: Edges::all(Border::NONE),
                padding: Edges::all(0.0),
                ..style.clone()
            }
        } else {
            style.clone()
        };
        let start = self.open(&frame, &[], x, measure);
        let (left, width) = frame.content_box(x, measure);
        let rows: Vec<&Row> = rows(head, body).collect();
        let table = Table::new(styles, style, &rows, collapse);
        if table.columns > 0 {
            let grid = table.grid(left, width);
            if grid.overflows {
                self.warn_at(
                    "The columns of a table are wider than the page. The table runs past the \
                     right margin.",
                    position,
                );
            }
            let mut first = true;
            for (index, row) in rows.iter().enumerate() {
                self.row(&table, &grid, index, row, head.len(), &mut first);
            }
        }
        self.close(&frame, start);
    }

    /// One row as one fragment: its cells set against their columns,
    /// the rules and backgrounds around them, and where a page can end
    /// above it.
    fn row(
        &mut self,
        table: &Table<'_>,
        grid: &Grid,
        index: usize,
        row: &Row,
        headers: usize,
        first: &mut bool,
    ) {
        let style = table.rows[index];
        let above = if table.collapse && index == 0 {
            table.thickness(0)
        } else {
            0.0
        };
        let below = if table.collapse {
            table.thickness(index + 1)
        } else {
            0.0
        };

        let mut cells = Vec::new();
        let mut anchors = Vec::new();
        let mut marks = None;
        let mut height = 0.0f32;
        for (column, cell) in row.cells.iter().enumerate() {
            let Some(cell_style) = table.cells[index][column] else {
                continue;
            };
            let border = table.border(cell_style);
            let padding = cell_style.padding;
            let x = grid.x[column] + border.left + padding.left;
            let measure = (grid.widths[column] - border.inline() - padding.inline()).max(0.0);
            let mut content = self.cell(&cell.blocks, x, measure);
            anchors.append(&mut content.anchors);
            gather(&mut marks, content.marks.take());
            let top = border.top + padding.top;
            height = height.max(top + content.height + padding.bottom + border.bottom);
            cells.push((column, cell_style, top, content));
        }

        let mut items = Vec::new();
        if let Some(color) = style.background_color {
            rect(&mut items, grid.left, above, grid.width, height, color);
        }
        for (column, cell_style, _, _) in &cells {
            let decoration = Decoration {
                x: grid.x[*column],
                width: grid.widths[*column],
                above: 0.0,
                below: 0.0,
                border: table.border(cell_style),
                colors: inks(cell_style),
                background: cell_style.background_color,
                cloned: false,
            };
            items.extend(
                Painted {
                    decoration,
                    top: above,
                    bottom: above + height,
                    cut_above: false,
                    cut_below: false,
                }
                .items((0.0, 0.0)),
            );
        }
        if table.collapse {
            if index == 0 {
                table.across(grid, 0, 0.0, &mut items);
            }
            table.across(grid, index + 1, above + height, &mut items);
            for (column, stroke) in table.down[index].iter().enumerate() {
                if let Some(stroke) = stroke {
                    let (x, _) = grid.rules[column];
                    rect(&mut items, x, above, stroke.width, height, stroke.color);
                }
            }
        }
        for (_, _, top, mut content) in cells {
            shift(&mut content.items, 0.0, above + top);
            items.append(&mut content.items);
        }

        let height = above + height + below;
        let page = self
            .paginator
            .styles
            .default_page()
            .geometry
            .content_size()
            .1;
        if height > page {
            self.warn_at(
                "A table row is taller than the page. The row runs past the bottom of the page.",
                row.position,
            );
        }

        let head = index < headers;
        let mut fragment = Fragment::plain(
            0.0,
            height,
            Piece::Row(Box::new(TableRow {
                items,
                opens: index == 0,
                head,
                repeats: headers > 0 && !head,
            })),
        );
        self.ask(style.break_before);
        gather(&mut self.pending_marks, marks);
        if !*first {
            // The header rows stay together, and the first body row
            // stays with them.
            let kept = index <= headers;
            fragment.break_before = match std::mem::replace(&mut self.pending, BreakPoint::Allowed)
            {
                BreakPoint::Allowed if kept => BreakPoint::Forbidden,
                asked => asked,
            };
            fragment.marks = self.pending_marks.take();
            // An anchor binds to the fragment after it, so the table
            // opens and closes on a row.
            for node in &anchors {
                self.anchor(*node);
            }
            anchors.clear();
        }
        self.emit(first, fragment);
        for node in anchors {
            self.anchor(node);
        }
        self.ask(style.break_after);
    }

    /// What one cell's blocks come to: what they paint, from the top
    /// of the cell's content box, and how tall they stand.
    fn cell(&mut self, blocks: &[Block], x: f32, measure: f32) -> Content {
        let mut inner = Builder::new(self.paginator, self.source);
        inner.blocks(blocks, x, measure);
        let mut marks = None;
        let mut anchors = Vec::new();
        let mut placed = Vec::new();
        let mut cursor = 0.0f32;
        for fragment in &inner.fragments {
            if let Piece::Anchor(node) = fragment.piece {
                anchors.push(node);
                continue;
            }
            gather(&mut marks, fragment.marks.clone());
            let top = cursor + fragment.lead + fragment.fixed;
            placed.push((top, fragment));
            cursor = top + fragment.height;
        }
        gather(&mut marks, inner.pending_marks.take());
        let mut items = decorate(&placed);
        for (top, fragment) in &placed {
            items.append(&mut self.paginator.fragment_items(fragment, 0.0, *top));
        }
        Content {
            items,
            height: cursor + inner.margin + inner.fixed,
            anchors,
            marks,
        }
    }

    /// Records a diagnostic at a place in the source.
    fn warn_at(&self, message: &str, position: Option<SourcePos>) {
        let at = origin(self.source, position);
        self.paginator
            .warn(message.to_string(), (!at.is_empty()).then_some(at));
    }
}

/// What one cell's blocks come to.
struct Content {
    /// What they paint, from the top of the cell's content box.
    items: Vec<DrawItem>,
    /// How tall they stand, the margins inside the cell included.
    height: f32,
    /// The images the sheet lifted out of the flow from inside the
    /// cell.
    anchors: Vec<NodeId>,
    /// What the blocks set for the page furniture.
    marks: Option<Box<Marks>>,
}

/// The decorated blocks inside one cell, as the rects they paint. A
/// row is never split, so no box inside it is cut.
fn decorate(placed: &[(f32, &Fragment)]) -> Vec<DrawItem> {
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
    boxes
        .iter()
        .flat_map(|box_| box_.items((0.0, 0.0)))
        .collect()
}

/// Adds what one fragment set for the page furniture to what is
/// gathered already.
fn gather(into: &mut Option<Box<Marks>>, from: Option<Box<Marks>>) {
    let Some(from) = from else {
        return;
    };
    let marks = into.get_or_insert_with(Box::default);
    marks.strings.extend(from.strings);
    marks.page_number = from.page_number.or(marks.page_number);
}

/// What each border edge of one element is painted in, `currentColor`
/// resolved.
fn inks(style: &ComputedStyle) -> Edges<Color> {
    let ink = |edge: Border| edge.color.unwrap_or(style.color);
    Edges {
        top: ink(style.border.top),
        right: ink(style.border.right),
        bottom: ink(style.border.bottom),
        left: ink(style.border.left),
    }
}

/// A filled rect, where it has an area to fill.
fn rect(items: &mut Vec<DrawItem>, x: f32, y: f32, w: f32, h: f32, color: Color) {
    if w > 0.0 && h > 0.0 {
        items.push(DrawItem::Rect { x, y, w, h, color });
    }
}

/// One stretch of a collapsed rule: how thick it is and what it is
/// painted in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Stroke {
    width: f32,
    color: Color,
}

/// The rule drawn where several borders meet in a collapsed table.
///
/// The widest border wins. Of two as wide, the one given first wins,
/// so a caller gives a cell's before a row's and a row's before the
/// table's, and of two cells the one above or to the left first. A
/// border that is not drawn gives way to any border that is.
fn winner(claims: &[(Border, &ComputedStyle)]) -> Option<Stroke> {
    let mut best: Option<Stroke> = None;
    for (border, style) in claims {
        let width = border.used();
        if width > 0.0 && best.is_none_or(|best| width > best.width) {
            best = Some(Stroke {
                width,
                color: border.color.unwrap_or(style.color),
            });
        }
    }
    best
}

/// A table's rows and the styles that decide its grid.
struct Table<'s> {
    style: &'s ComputedStyle,
    /// The style of every row, header rows first.
    rows: Vec<&'s ComputedStyle>,
    /// The style of every cell, row by row, and `None` where a row has
    /// fewer cells than the table has columns.
    cells: Vec<Vec<Option<&'s ComputedStyle>>>,
    columns: usize,
    collapse: bool,
    /// Collapsed only: the rule to the left of each column, row by
    /// row, with one more for the right side.
    down: Vec<Vec<Option<Stroke>>>,
    /// Collapsed only: the rule over each column above each row, with
    /// one more under the last row.
    across: Vec<Vec<Option<Stroke>>>,
}

impl<'s> Table<'s> {
    fn new(
        styles: &'s StyleTree,
        style: &'s ComputedStyle,
        rows: &[&Row],
        collapse: bool,
    ) -> Table<'s> {
        let columns = rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        let mut table = Table {
            style,
            rows: rows.iter().map(|row| styles.style(row.id)).collect(),
            cells: rows
                .iter()
                .map(|row| {
                    (0..columns)
                        .map(|column| row.cells.get(column).map(|cell| styles.style(cell.id)))
                        .collect()
                })
                .collect(),
            columns,
            collapse,
            down: Vec::new(),
            across: Vec::new(),
        };
        if collapse {
            table.down = (0..rows.len())
                .map(|row| {
                    (0..=columns)
                        .map(|column| table.down_at(row, column))
                        .collect()
                })
                .collect();
            table.across = (0..=rows.len())
                .map(|rule| {
                    (0..columns)
                        .map(|column| table.across_at(rule, column))
                        .collect()
                })
                .collect();
        }
        table
    }

    fn cell(&self, row: usize, column: usize) -> Option<&'s ComputedStyle> {
        self.cells.get(row)?.get(column).copied().flatten()
    }

    /// The rule to the left of `column` in `row`, from the borders
    /// that meet there.
    fn down_at(&self, row: usize, column: usize) -> Option<Stroke> {
        let mut claims = Vec::new();
        if column > 0
            && let Some(style) = self.cell(row, column - 1)
        {
            claims.push((style.border.right, style));
        }
        if let Some(style) = self.cell(row, column) {
            claims.push((style.border.left, style));
        }
        let own = self.rows[row];
        if column == 0 {
            claims.push((own.border.left, own));
            claims.push((self.style.border.left, self.style));
        }
        if column == self.columns {
            claims.push((own.border.right, own));
            claims.push((self.style.border.right, self.style));
        }
        winner(&claims)
    }

    /// The rule over `column` above row `rule`, or under the last row
    /// where `rule` is past it.
    fn across_at(&self, rule: usize, column: usize) -> Option<Stroke> {
        let mut claims = Vec::new();
        let last = self.rows.len();
        if rule > 0
            && let Some(style) = self.cell(rule - 1, column)
        {
            claims.push((style.border.bottom, style));
        }
        if let Some(style) = self.cell(rule, column) {
            claims.push((style.border.top, style));
        }
        if rule > 0 {
            claims.push((self.rows[rule - 1].border.bottom, self.rows[rule - 1]));
        }
        if rule < last {
            claims.push((self.rows[rule].border.top, self.rows[rule]));
        }
        if rule == 0 {
            claims.push((self.style.border.top, self.style));
        }
        if rule == last {
            claims.push((self.style.border.bottom, self.style));
        }
        winner(&claims)
    }

    /// How much height one rule across the table takes: its thickest
    /// stretch.
    fn thickness(&self, rule: usize) -> f32 {
        self.across[rule]
            .iter()
            .flatten()
            .map(|stroke| stroke.width)
            .fold(0.0, f32::max)
    }

    /// How much width one rule down the table takes: its thickest
    /// stretch, over every row.
    fn breadth(&self, column: usize) -> f32 {
        self.down
            .iter()
            .filter_map(|row| row[column])
            .map(|stroke| stroke.width)
            .fold(0.0, f32::max)
    }

    /// The border a cell draws around its own content. Collapsed, the
    /// rules do that instead, and a cell draws none.
    fn border(&self, style: &ComputedStyle) -> Edges {
        if self.collapse {
            Edges::all(0.0)
        } else {
            style.border.widths()
        }
    }

    /// Paints the rule above row `rule` across the grid, its top at
    /// `y`. Each stretch runs over its column and the rule down the
    /// left of it, and the last over the rule down the right side as
    /// well, so the corners fall to the rule across.
    fn across(&self, grid: &Grid, rule: usize, y: f32, items: &mut Vec<DrawItem>) {
        for (column, stroke) in self.across[rule].iter().enumerate() {
            let Some(stroke) = stroke else { continue };
            let (from, _) = grid.rules[column];
            let to = match grid.rules.get(column + 1) {
                Some((x, width)) if column + 1 == self.columns => x + width,
                Some((x, _)) => *x,
                None => from,
            };
            rect(items, from, y, to - from, stroke.width, stroke.color);
        }
    }

    /// Where the columns and the rules between them fall across a
    /// table `width` wide, from `left`.
    fn grid(&self, left: f32, width: f32) -> Grid {
        let columns = self.columns;
        let breadths: Vec<f32> = (0..=columns)
            .map(|column| {
                if self.collapse {
                    self.breadth(column)
                } else {
                    0.0
                }
            })
            .collect();
        let available = (width - breadths.iter().sum::<f32>()).max(0.0);
        // A cell's `width` is the width of its content, so the column
        // is that and the padding and border around it.
        let asked: Vec<Option<f32>> = (0..columns)
            .map(|column| {
                let style = self.cell(0, column)?;
                let content = style.width.resolve(width)?;
                Some(content + style.padding.inline() + self.border(style).inline())
            })
            .collect();
        let fixed: f32 = asked.iter().flatten().sum();
        let autos = asked.iter().filter(|asked| asked.is_none()).count();
        let spare = available - fixed;
        let widths: Vec<f32> = asked
            .iter()
            .map(|asked| match asked {
                // Every column named its width, and the table is wider
                // than they come to: each takes a share of the rest.
                Some(width) if autos == 0 => width + spare.max(0.0) / columns as f32,
                Some(width) => *width,
                None => (spare / autos as f32).max(0.0),
            })
            .collect();
        let mut x = left;
        let mut starts = Vec::with_capacity(columns);
        let mut rules = Vec::with_capacity(columns + 1);
        for (column, width) in widths.iter().enumerate() {
            rules.push((x, breadths[column]));
            x += breadths[column];
            starts.push(x);
            x += width;
        }
        rules.push((x, breadths[columns]));
        x += breadths[columns];
        Grid {
            x: starts,
            widths,
            rules,
            left,
            width: x - left,
            overflows: spare < -0.01,
        }
    }
}

/// Where a table's columns and rules fall across its width, from the
/// content box's leading edge.
struct Grid {
    /// Leading edge of each column.
    x: Vec<f32>,
    /// Width of each column, between the rules on either side of it.
    widths: Vec<f32>,
    /// Where each rule down the table starts, and the width it takes:
    /// one to the left of each column and one down the right side.
    /// Separated, every one takes none.
    rules: Vec<(f32, f32)>,
    /// Leading edge of the grid.
    left: f32,
    /// Width of the grid, rules included.
    width: f32,
    /// Whether the columns the sheet sized are wider than the table.
    overflows: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A border with nothing drawn on it gives way to any border that
    /// is drawn, the widest border wins, and of two as wide the first
    /// given wins.
    #[test]
    fn the_widest_border_draws_the_collapsed_rule() {
        let style = ComputedStyle::initial();
        let solid = |width: f32, color: Color| Border {
            style: crate::style::BorderStyle::Solid,
            width,
            color: Some(color),
        };
        let red = Color::rgb(200, 0, 0);
        let blue = Color::rgb(0, 0, 200);
        assert_eq!(winner(&[(Border::NONE, &style)]), None);
        assert_eq!(
            winner(&[(Border::NONE, &style), (solid(1.0, blue), &style)]),
            Some(Stroke {
                width: 1.0,
                color: blue
            }),
        );
        assert_eq!(
            winner(&[(solid(1.0, red), &style), (solid(2.0, blue), &style)]),
            Some(Stroke {
                width: 2.0,
                color: blue
            }),
        );
        assert_eq!(
            winner(&[(solid(1.0, red), &style), (solid(1.0, blue), &style)]),
            Some(Stroke {
                width: 1.0,
                color: red
            }),
        );
        // A border with no colour of its own is painted in the colour
        // of the element that drew it.
        let inked = ComputedStyle {
            color: blue,
            ..ComputedStyle::initial()
        };
        let plain = Border {
            color: None,
            ..solid(1.0, red)
        };
        assert_eq!(
            winner(&[(plain, &inked)]).map(|stroke| stroke.color),
            Some(blue)
        );
    }
}
