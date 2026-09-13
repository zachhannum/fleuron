//! What a host asks about one thing on a page: what styled it and
//! where it landed, and what is under a point.

use crate::content::NodeId;
use crate::layout::Paginator;
use crate::pages::{DrawItem, PageBox};
use crate::style::{Inspection, MarginBox, element_of};

use super::Session;

impl Session<'_> {
    /// The element one node stands for, the rules that matched it and
    /// what it computed to, and its border box on each page it
    /// reaches. A text node answers for the element that holds it.
    ///
    /// Nothing for a node the book does not hold, or for the id the
    /// engine writes its own text under. An id names a node only
    /// until the next edit, so the answer is about the book as it
    /// stands.
    pub fn inspect(&mut self, node: NodeId) -> Option<Inspection> {
        self.update();
        let sheets = self.sheets.as_ref()?;
        let mut inspection = sheets.inspect(&self.book, &self.styles, node)?;
        let element = inspection.node?;
        inspection.boxes = self.boxes_of(element);
        Some(inspection)
    }

    /// The same for one margin box of the page at `index`, counting
    /// from 0: the page selector it answers to, the `@page` rules that
    /// set it, and the box its text takes.
    ///
    /// Nothing for a page the book does not have, for a blank page,
    /// and for a box no rule for that page names.
    pub fn inspect_margin_box(&mut self, index: usize, which: MarginBox) -> Option<Inspection> {
        self.update();
        let sheets = self.sheets.as_ref()?;
        let paginator = Paginator::with_contours(
            self.registry.get(),
            &self.styles,
            self.assets.get(),
            &self.contours,
        );
        let (query, area) = paginator.margin_box(&self.infos, index, which)?;
        let mut inspection = sheets.inspect_margin_box(&self.styles, query, which)?;
        inspection.boxes = area.into_iter().collect();
        Some(inspection)
    }

    /// The innermost element at a point on the page at `index`, in
    /// points from its top-left corner: the element that holds the
    /// text there, or else the innermost block whose border box holds
    /// the point, padding and empty space included.
    ///
    /// Nothing outside every box, and nothing for a page the book does
    /// not have.
    pub fn hit(&mut self, index: usize, x: f32, y: f32) -> Option<NodeId> {
        self.update();
        let page = self.output.as_ref()?.pages.get(index)?;
        let text = page
            .items
            .iter()
            .filter_map(|item| self.run_box(index, item))
            .find(|(_, area)| area.contains(x, y));
        if let Some((node, _)) = text {
            return element_of(&self.book, node);
        }
        self.boxes
            .iter()
            .filter(|(_, area)| area.page as usize == index && area.contains(x, y))
            .map(|(node, _)| *node)
            .max()
    }

    /// Where one element is on the pages: the border box of a block on
    /// each page and column it reaches, or for an element with no box
    /// of its own, such as emphasis, the area its text covers on each
    /// page.
    fn boxes_of(&self, element: NodeId) -> Vec<PageBox> {
        let blocks: Vec<PageBox> = self
            .boxes
            .iter()
            .filter(|(node, _)| *node == element)
            .map(|(_, area)| *area)
            .collect();
        if !blocks.is_empty() {
            return blocks;
        }
        let (Some(held), Some(output)) = (self.book.subtree(element), &self.output) else {
            return Vec::new();
        };
        let mut boxes = Vec::new();
        for (index, page) in output.pages.iter().enumerate() {
            let covered = page
                .items
                .iter()
                .filter(|item| {
                    matches!(item, DrawItem::Text { origin: Some(origin), .. }
                        if held.contains(&origin.node.get()))
                })
                .filter_map(|item| self.run_box(index, item))
                .map(|(_, area)| area)
                .reduce(|one, other| {
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
                });
            boxes.extend(covered);
        }
        boxes
    }

    /// The node a run of text was written in, and the area the run
    /// covers: its glyphs across, and its face's ascent and descent
    /// down. Nothing for text the engine wrote itself.
    fn run_box(&self, index: usize, item: &DrawItem) -> Option<(NodeId, PageBox)> {
        let DrawItem::Text {
            y,
            font_id,
            size,
            glyphs,
            origin: Some(origin),
            ..
        } = item
        else {
            return None;
        };
        let registry = self.registry.get();
        let metrics = registry.metrics(*font_id)?;
        let scale = size / metrics.units_per_em as f32;
        let (first, last) = (glyphs.first()?, glyphs.last()?);
        let advance = registry.advance_width(*font_id, last.id).unwrap_or(0) as f32 * scale;
        let (left, right) = (first.x.min(last.x), first.x.max(last.x + advance));
        let top = y - metrics.ascender as f32 * scale;
        let bottom = y - metrics.descender as f32 * scale;
        Some((
            origin.node,
            PageBox {
                page: index as u32,
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Attributes, Block, Inline, block_id};
    use crate::pages::Side;
    use crate::session::testing::{book, paragraph, prose, registry, section, sheets};
    use crate::style::Color;

    fn text(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    /// One chapter: a paragraph with emphasis in it, and prose after.
    fn chapter(css: &str, first: Block) -> Session<'static> {
        let mut session = Session::new(registry());
        session.set_content(book(vec![section(
            "one.md",
            [vec![first], prose("alpha", 3)].concat(),
        )]));
        session.set_style(sheets(css));
        session
    }

    fn emphatic() -> Block {
        Block::Paragraph {
            id: NodeId::UNASSIGNED,
            inlines: vec![
                text("Lo, "),
                Inline::Emphasis {
                    id: NodeId::UNASSIGNED,
                    children: vec![text("behold")],
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                },
                text(" the sea."),
            ],
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    const TINT: Color = Color::rgb(0xee, 0xdd, 0xcc);

    /// The rects a page paints in `TINT`, as boxes.
    fn tinted(session: &mut Session<'_>) -> Vec<PageBox> {
        let output = session.preview();
        output
            .pages
            .iter()
            .enumerate()
            .flat_map(|(index, page)| {
                page.items.iter().filter_map(move |item| match item {
                    DrawItem::Rect {
                        x, y, w, h, color, ..
                    } if *color == TINT => Some(PageBox {
                        page: index as u32,
                        x: *x,
                        y: *y,
                        width: *w,
                        height: *h,
                    }),
                    _ => None,
                })
            })
            .collect()
    }

    fn close(one: &PageBox, other: &PageBox) -> bool {
        one.page == other.page
            && [
                (one.x, other.x),
                (one.y, other.y),
                (one.width, other.width),
                (one.height, other.height),
            ]
            .iter()
            .all(|(a, b)| (a - b).abs() < 0.01)
    }

    const PADDED: &str = "p:first-child { background-color: #eeddcc; padding: 12pt; \
                          border: 3pt solid black; margin-bottom: 24pt }";

    #[test]
    fn a_block_answers_with_its_border_box_in_page_points() {
        let mut session = chapter(PADDED, emphatic());
        let node = block_id(&session.book().sections[0].blocks[0]);
        let painted = tinted(&mut session);
        let inspection = session.inspect(node).expect("the paragraph");

        assert_eq!(inspection.element, "p");
        assert_eq!(painted.len(), 1, "one background");
        assert_eq!(inspection.boxes.len(), 1);
        assert!(
            close(&inspection.boxes[0], &painted[0]),
            "{:?} is not the background at {:?}",
            inspection.boxes[0],
            painted[0]
        );

        let emphasis = NodeId::new(node.get() + 2);
        let inline = session.inspect(emphasis).expect("the emphasis");
        assert_eq!(inline.element, "em");
        assert_eq!(inline.boxes.len(), 1, "emphasis covers its own text");
        let (outer, inner) = (&inspection.boxes[0], &inline.boxes[0]);
        assert!(inner.x > outer.x && inner.x + inner.width < outer.x + outer.width);
        assert!(inner.y > outer.y && inner.y + inner.height < outer.y + outer.height);
    }

    #[test]
    fn a_block_split_across_two_pages_answers_with_two_boxes() {
        let long = paragraph(&"alpha ".repeat(700));
        let mut session = chapter("p:first-child { background-color: #eeddcc }", long);
        let node = block_id(&session.book().sections[0].blocks[0]);
        let painted = tinted(&mut session);
        let inspection = session.inspect(node).expect("the paragraph");

        assert!(painted.len() >= 2, "the paragraph runs onto a second page");
        assert_eq!(inspection.boxes.len(), painted.len());
        for (area, background) in inspection.boxes.iter().zip(&painted) {
            assert!(close(area, background), "{area:?} against {background:?}");
        }
        let pages: Vec<u32> = inspection.boxes.iter().map(|area| area.page).collect();
        assert!(
            pages.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "{pages:?}"
        );
    }

    #[test]
    fn a_margin_box_answers_with_its_page_selector_its_rules_and_its_box() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![section("one.md", prose("alpha", 12))]));
        session.set_style(sheets("@page :left { @top-left { content: \"Left\" } }"));
        let output = session.preview();
        let (index, run) = output
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.side == Side::Verso)
            .find_map(|(index, page)| {
                page.items.iter().find_map(|item| match item {
                    DrawItem::Text { x, y, text, .. } if text == "Left" => Some((index, (*x, *y))),
                    _ => None,
                })
            })
            .expect("a left page shows the box");
        let recto = output
            .pages
            .iter()
            .position(|page| page.side == Side::Recto)
            .expect("a right page");

        let inspection = session
            .inspect_margin_box(index, MarginBox::TopLeft)
            .expect("the left page names @top-left");
        assert_eq!(inspection.element, "@top-left");
        assert_eq!(
            inspection.page.as_deref(),
            Some("@page chapter:left"),
            "the built-in sheet sets a chapter on the page named chapter"
        );
        let rule = inspection
            .rules
            .iter()
            .find(|rule| rule.sheet == "test.css")
            .expect("the author's rule");
        assert_eq!(
            (rule.selector.as_str(), rule.line, rule.column),
            ("@page :left", 1, 1)
        );
        assert_eq!(rule.declarations[0].property, "content");
        assert!(rule.declarations[0].applied);
        assert_eq!(inspection.boxes.len(), 1);
        assert_eq!(inspection.boxes[0].page, index as u32);
        assert!(inspection.boxes[0].contains(run.0 + 1.0, run.1 - 1.0));

        assert_eq!(session.inspect_margin_box(recto, MarginBox::TopLeft), None);
    }

    #[test]
    fn a_synthesized_or_unknown_node_answers_nothing() {
        let mut session = chapter(PADDED, emphatic());
        assert_eq!(session.inspect(NodeId::UNASSIGNED), None);
        assert_eq!(session.inspect(NodeId::new(u32::MAX)), None);
        assert_eq!(session.inspect_margin_box(9999, MarginBox::TopLeft), None);
    }

    #[test]
    fn a_point_answers_with_the_innermost_element_there() {
        let mut session = chapter(PADDED, emphatic());
        let node = block_id(&session.book().sections[0].blocks[0]);
        let area = session.inspect(node).expect("the paragraph").boxes[0];

        assert_eq!(
            session.hit(0, area.x + 6.0, area.y + 6.0),
            Some(node),
            "the padding is the paragraph's, though no text is there"
        );
        let emphasis = NodeId::new(node.get() + 2);
        let word = session.inspect(emphasis).expect("the emphasis").boxes[0];
        assert_eq!(
            session.hit(0, word.x + word.width / 2.0, word.y + word.height / 2.0),
            Some(emphasis)
        );
        let plain = session.inspect(NodeId::new(node.get() + 1)).expect("text");
        assert_eq!(
            plain.node,
            Some(node),
            "plain text answers for its paragraph"
        );

        let section = session.book().sections[0].id;
        let under = session.inspect(section).expect("the chapter").boxes[0];
        assert_eq!(
            session.hit(0, under.x + 1.0, area.y + area.height + 12.0),
            Some(section),
            "the margin below a paragraph is the chapter's"
        );
    }

    #[test]
    fn a_point_outside_every_box_answers_nothing() {
        let mut session = chapter(PADDED, emphatic());
        assert_eq!(session.hit(0, 1.0, 1.0), None, "the corner of the page");
        assert_eq!(session.hit(9999, 100.0, 100.0), None, "no such page");
    }

    #[test]
    fn boxes_follow_a_chapter_that_an_edit_renumbered() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![
            section("one.md", prose("alpha", 4)),
            section("two.md", prose("beta", 4)),
        ]));
        session.preview();
        session.replace_source("one.md", vec![section("one.md", prose("alpha", 9))]);
        let node = block_id(&session.book().sections[1].blocks[0]);
        let folios = session.folios(&[node])[0].expect("the chapter reaches a page");
        let boxes = session.inspect(node).expect("the paragraph").boxes;

        assert!(!boxes.is_empty());
        assert!(
            boxes
                .iter()
                .all(|area| (folios.at..folios.at + folios.count).contains(&area.page)),
            "{boxes:?} against {folios:?}"
        );
    }
}
