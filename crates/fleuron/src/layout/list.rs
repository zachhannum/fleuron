//! A list: each item a block inside the list's content box, and the
//! marker of each item to the left of it, on the baseline of the
//! item's first line.

use crate::content::{GeneratedBox, ListItem, NodeId, SourcePos};
use crate::style::{Break, ComputedStyle};

use super::build::{Builder, children};
use super::fragment::{Fragment, Marker, Piece};

impl Builder<'_, '_> {
    /// One list, at `x` from the content box's leading edge and
    /// breaking to `measure`. `start` is the number of its first item.
    pub(super) fn list(
        &mut self,
        id: NodeId,
        start: u32,
        items: &[ListItem],
        position: Option<SourcePos>,
        x: f32,
        measure: f32,
    ) {
        let styles = self.paginator.styles;
        let style = styles.style(id).clone();
        let open = self.open(id, &style, &[], x, measure);
        let (inner, narrowed) = style.content_box(x, measure);
        self.pseudo(id, GeneratedBox::Before, position, inner, narrowed);
        let last = items.len().saturating_sub(1);
        for (index, item) in items.iter().enumerate() {
            // A page does not end after the first item or before the
            // last one, so neither is left alone on a page.
            if index == 1 || (index > 0 && index == last) {
                self.ask(Break::Avoid);
            }
            self.item(item, start.saturating_add(index as u32), inner, narrowed);
        }
        self.pseudo(id, GeneratedBox::After, position, inner, narrowed);
        self.close(&style, open);
    }

    /// One item of a list, numbered `number`.
    fn item(&mut self, item: &ListItem, number: u32, x: f32, measure: f32) {
        let styles = self.paginator.styles;
        if item.attributes.id.is_some() {
            self.name(item.id);
        }
        let style = styles.style(item.id).clone();
        let open = self.open(item.id, &style, &[], x, measure);
        let (inner, narrowed) = style.content_box(x, measure);
        self.blocks(
            children(styles, item.id, &item.blocks, item.position),
            inner,
            narrowed,
        );
        if let Some(marker) = self.marker(&style, number, x, measure) {
            self.hang(open, marker);
        }
        self.close(&style, open);
    }

    /// The marker of the item numbered `number`, shaped in the item's
    /// style. It ends where the item's border box starts.
    fn marker(&self, style: &ComputedStyle, number: u32, x: f32, measure: f32) -> Option<Marker> {
        let text = style.list_style_type.marker(number)?;
        let line = self.paginator.line_of(&text, style.paragraph())?;
        let (left, _) = style.border_box(x, measure);
        let x = left - self.paginator.line_width(&line);
        Some(Marker { line, x })
    }

    /// Puts a marker on the first fragment its item emitted, from
    /// `start`. An item that emitted nothing emits the height of the
    /// marker to set it on.
    fn hang(&mut self, start: usize, marker: Marker) {
        let first = self.fragments[start..]
            .iter_mut()
            .find(|fragment| !matches!(fragment.piece, Piece::Anchor(_)));
        match first {
            Some(fragment) => fragment
                .markers
                .get_or_insert_with(Box::default)
                .insert(0, marker),
            None => {
                let mut blank = Fragment::plain(0.0, marker.line.box_.height, Piece::Blank);
                blank.markers = Some(Box::new(vec![marker]));
                self.emit(&mut true, blank);
            }
        }
    }
}
