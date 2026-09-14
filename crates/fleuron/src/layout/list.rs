//! A list: each item a block inside the list's content box.

use crate::content::{GeneratedBox, ListItem, NodeId, SourcePos};

use super::build::{Builder, children};

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
        for (index, item) in items.iter().enumerate() {
            self.item(item, start.saturating_add(index as u32), inner, narrowed);
        }
        self.pseudo(id, GeneratedBox::After, position, inner, narrowed);
        self.close(&style, open);
    }

    /// One item of a list, numbered `number`.
    fn item(&mut self, item: &ListItem, _number: u32, x: f32, measure: f32) {
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
        self.close(&style, open);
    }
}
