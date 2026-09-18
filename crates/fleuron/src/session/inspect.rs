//! What a host asks about one thing on a page: what styled it and
//! where it landed, and what is under a point.

use crate::content::{NodeId, PseudoElement};
use crate::layout::{Paginator, run_area};
use crate::pages::{DrawItem, PageBox};
use crate::style::{Inspection, MarginBox, element_of};

use super::Session;

impl Session<'_> {
    /// The element one node stands for, the rules that matched it and
    /// what it computed to, and its border box on each page it
    /// reaches. A text node answers for the element that holds it. A
    /// pseudo-element answers for itself, with its own boxes: the box
    /// `::before` or `::after` generates on a block, or else the area
    /// its text covers on each page.
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
    /// the point, padding and empty space included. Where two such
    /// things overlap, the one painted later answers. A pseudo-element
    /// answers with its own id: a drop cap, the line a paragraph opens
    /// on, and the box or the text of `::before` and `::after`. An
    /// inline element on the opening line answers for itself.
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
            .rfind(|(_, _, area)| area.contains(x, y));
        if let Some((node, pseudo, _)) = text {
            let element = element_of(&self.book, node);
            let inside = |pseudo: NodeId| match pseudo.pseudo_element() {
                Some((line, PseudoElement::FirstLine)) => element == Some(line),
                _ => true,
            };
            return pseudo.filter(|pseudo| inside(*pseudo)).or(element);
        }
        let under: Vec<NodeId> = self
            .boxes
            .iter()
            .filter(|(_, area)| area.page as usize == index && area.contains(x, y))
            .map(|(node, _)| *node)
            .collect();
        // A box that holds another box under the point is not the
        // innermost one. A block against the page is recorded after the
        // blocks it covers. A generated box holds nothing but its text.
        under
            .iter()
            .rfind(|node| {
                let held = match node.pseudo_element() {
                    Some(_) => None,
                    None => self.book.subtree(**node),
                };
                let held = held.unwrap_or_default();
                !under
                    .iter()
                    .any(|other| other != *node && held.contains(&other.element().get()))
            })
            .copied()
    }

    /// Where one element or pseudo-element is on the pages: the border
    /// box of a block on each page and column it reaches, or for one
    /// with no box of its own, such as emphasis or a drop cap, the
    /// area its text covers on each page.
    fn boxes_of(&self, node: NodeId) -> Vec<PageBox> {
        let blocks: Vec<PageBox> = self
            .boxes
            .iter()
            .filter(|(id, _)| *id == node)
            .map(|(_, area)| *area)
            .collect();
        if !blocks.is_empty() {
            return blocks;
        }
        let Some(output) = &self.output else {
            return Vec::new();
        };
        let held = self.book.subtree(node);
        let covers = |origin: NodeId, pseudo: Option<NodeId>| match node.pseudo_element() {
            Some(_) => pseudo == Some(node),
            None => held
                .as_ref()
                .is_some_and(|held| held.contains(&origin.element().get())),
        };
        let mut boxes = Vec::new();
        for (index, page) in output.pages.iter().enumerate() {
            let covered = page
                .items
                .iter()
                .filter_map(|item| self.run_box(index, item))
                .filter(|(origin, pseudo, _)| covers(*origin, *pseudo))
                .map(|(_, _, area)| area)
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

    /// The node a run of text was written in, the pseudo-element it
    /// was cut from, and the area the run covers.
    fn run_box(&self, index: usize, item: &DrawItem) -> Option<(NodeId, Option<NodeId>, PageBox)> {
        run_area(self.registry.get(), index, item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Attributes, Block, Inline, PseudoElement, block_id};
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

    /// A generated box records its border box under its own id. Its
    /// padding and its text answer with that id, and its inspection
    /// answers with that box.
    #[test]
    fn a_generated_box_records_its_box_and_answers_for_itself() {
        let css = "p:first-child::after { content: \"Fin\"; padding: 6pt; \
                   background-color: #eeddcc }";
        let mut session = chapter(css, emphatic());
        let node = block_id(&session.book().sections[0].blocks[0]);
        let painted = tinted(&mut session);
        let generated = session
            .styles
            .pseudo_element(node, PseudoElement::After)
            .expect("the paragraph generates a box");

        let recorded: Vec<PageBox> = session
            .boxes
            .iter()
            .filter(|(id, _)| *id == generated)
            .map(|(_, area)| *area)
            .collect();
        assert_eq!(painted.len(), 1, "one background");
        assert_eq!(recorded.len(), 1, "one box under the generated id");
        assert!(
            close(&recorded[0], &painted[0]),
            "{:?} is not the background at {:?}",
            recorded[0],
            painted[0]
        );

        let area = recorded[0];
        assert_eq!(
            session.hit(0, area.x + 2.0, area.y + 2.0),
            Some(generated),
            "the padding of the box answers for the box"
        );
        let (x, y, origin) = session
            .preview()
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .find_map(|item| match item {
                DrawItem::Text {
                    x, y, text, origin, ..
                } if text == "Fin" => Some((*x, *y, origin.clone())),
                _ => None,
            })
            .expect("the box sets its text");
        assert_eq!(origin.map(|origin| origin.node), Some(generated));
        assert_eq!(
            session.hit(0, x + 1.0, y - 2.0),
            Some(generated),
            "the text of the box answers for the box"
        );

        let inspection = session.inspect(generated).expect("the box");
        assert_eq!(inspection.node, Some(generated));
        assert_eq!(inspection.element, "p");
        assert_eq!(inspection.boxes.len(), 1);
        assert!(close(&inspection.boxes[0], &recorded[0]));
    }

    /// A chapter under an `h2`: a paragraph long enough to sink a drop
    /// cap into, with a link near its end.
    fn under_h2(css: &str) -> Session<'static> {
        under_h2_opening(css, "")
    }

    /// The same, with `opens` written before the paragraph's prose.
    fn under_h2_opening(css: &str, opens: &str) -> Session<'static> {
        let text = |value: &str| format!(r#"{{"type": "text", "value": "{value}"}}"#);
        let blocks = format!(
            r#"{{"type": "heading", "level": 2, "inlines": [{}]}},
               {{"type": "paragraph", "inlines": [{}, {{"type": "link",
                 "url": "https://example.com", "children": [{}]}}, {}]}}"#,
            text("A Voyage"),
            text(&format!(
                "{opens}{}",
                "my father had a small estate in nottinghamshire ".repeat(6)
            )),
            text("Lilliput"),
            text(" and more."),
        );
        from_json(&blocks, css)
    }

    /// The paragraph under the heading, the text node it opens with,
    /// and the link after that.
    fn opening(session: &Session<'_>) -> (NodeId, NodeId, NodeId) {
        let paragraph = block_id(&session.book().sections[0].blocks[1]);
        let text = NodeId::new(paragraph.get() + 1);
        (paragraph, text, NodeId::new(text.get() + 1))
    }

    /// The first run `pick` takes, and the area it covers.
    fn run_where(
        session: &mut Session<'_>,
        pick: impl Fn(&DrawItem) -> bool,
    ) -> (DrawItem, PageBox) {
        let (index, item) = session
            .preview()
            .pages
            .iter()
            .enumerate()
            .find_map(|(index, page)| {
                let item = page.items.iter().find(|item| pick(item))?;
                Some((index, item.clone()))
            })
            .expect("a run matches");
        let (_, _, area) = session.run_box(index, &item).expect("the run names a node");
        (item, area)
    }

    fn spelled(text: &'static str) -> impl Fn(&DrawItem) -> bool {
        move |item| matches!(item, DrawItem::Text { text: run, .. } if run.contains(text))
    }

    fn centre(area: &PageBox) -> (f32, f32) {
        (area.x + area.width / 2.0, area.y + area.height / 2.0)
    }

    fn author_selectors(inspection: &Inspection) -> Vec<&str> {
        inspection
            .rules
            .iter()
            .filter(|rule| rule.sheet == "test.css")
            .map(|rule| rule.selector.as_str())
            .collect()
    }

    const DROP_CAP: &str = "h2 + p::first-letter { initial-letter: 3 }";
    const SMALL_CAPS: &str = "h2 + p::first-line { font-variant-caps: small-caps }";
    const LINK: &str = "a::after { content: \" (link)\" } a { color: #333333 }";

    /// Acceptance: with `h2 + p::first-letter { initial-letter: 3 }`, a
    /// point on the drop cap hits the paragraph's `::first-letter` id.
    /// Its inspection lists that rule and `initial-letter: 3`.
    #[test]
    fn a_point_on_a_drop_cap_hits_its_first_letter() {
        let mut session = under_h2(DROP_CAP);
        let (paragraph, ..) = opening(&session);
        let letter = paragraph.pseudo(PseudoElement::FirstLetter);
        let (_, area) = run_where(
            &mut session,
            |item| matches!(item, DrawItem::Text { text, .. } if text == "m"),
        );
        let (x, y) = centre(&area);
        assert_eq!(session.hit(area.page as usize, x, y), Some(letter));

        let inspection = session.inspect(letter).expect("the drop cap");
        assert_eq!(author_selectors(&inspection), ["h2 + p::first-letter"]);
        let declaration = inspection
            .rules
            .iter()
            .flat_map(|rule| &rule.declarations)
            .find(|declaration| declaration.property == "initial-letter")
            .expect("the rule declares `initial-letter`");
        assert_eq!(declaration.value, "3");
        assert!(declaration.applied);
        assert_eq!(inspection.computed["initial-letter"], "3");
    }

    /// Acceptance: with `h2 + p::first-line { font-variant-caps:
    /// small-caps }`, a point on the first line after the drop cap
    /// hits the `::first-line` id. Its inspection lists that rule.
    #[test]
    fn a_point_on_the_first_line_hits_its_first_line() {
        let mut session = under_h2(&format!("{DROP_CAP} {SMALL_CAPS}"));
        let (paragraph, text, _) = opening(&session);
        let line = paragraph.pseudo(PseudoElement::FirstLine);
        let (_, area) = run_where(&mut session, |item| {
            matches!(item, DrawItem::Text { origin: Some(origin), .. }
                if origin.node == text && origin.range.start == 1)
        });
        let (x, y) = centre(&area);
        assert_eq!(session.hit(area.page as usize, x, y), Some(line));
        let inspection = session.inspect(line).expect("the first line");
        assert_eq!(author_selectors(&inspection), ["h2 + p::first-line"]);

        let (_, later) = run_where(&mut session, |item| {
            matches!(item, DrawItem::Text { origin: Some(origin), .. }
                if origin.node == text && origin.range.start > 150)
        });
        let (x, y) = centre(&later);
        assert_eq!(
            session.hit(later.page as usize, x, y),
            Some(paragraph),
            "a later line is the paragraph's"
        );
    }

    /// Acceptance: a point in the box of `h2::before` hits its id. Its
    /// inspection lists the `h2::before` rule, not only the rules for
    /// `h2`.
    #[test]
    fn a_point_in_the_box_of_h2_before_hits_its_id() {
        let css = "h2 { color: #333333 } \
                   h2::before { content: \"\"; height: 12pt; padding: 6pt; \
                   background-color: #eeddcc }";
        let mut session = under_h2(css);
        let heading = block_id(&session.book().sections[0].blocks[0]);
        let before = heading.pseudo(PseudoElement::Before);
        let painted = tinted(&mut session);
        assert_eq!(painted.len(), 1, "one background");
        let area = painted[0];
        assert_eq!(
            session.hit(area.page as usize, area.x + 2.0, area.y + 2.0),
            Some(before)
        );

        let inspection = session.inspect(before).expect("the box");
        assert_eq!(author_selectors(&inspection), ["h2::before"]);
        let heading = session.inspect(heading).expect("the heading");
        assert_eq!(author_selectors(&heading), ["h2"]);
    }

    /// Acceptance: a point on the text of `a::after` hits its id, and
    /// its inspection lists the `a::after` rule.
    #[test]
    fn a_point_on_the_text_of_a_after_hits_its_id() {
        let mut session = under_h2(LINK);
        let (.., link) = opening(&session);
        let after = link.pseudo(PseudoElement::After);
        let (_, area) = run_where(&mut session, spelled("(link)"));
        let (x, y) = centre(&area);
        assert_eq!(session.hit(area.page as usize, x, y), Some(after));

        let inspection = session.inspect(after).expect("the generated text");
        assert_eq!(author_selectors(&inspection), ["a::after"]);
        assert_eq!(inspection.computed["content"], "\" (link)\"");
        assert_eq!(inspection.boxes.len(), 1);
        assert!(close(&inspection.boxes[0], &area));
    }

    /// Acceptance: the box an inspection returns for a drop cap is the
    /// rectangle of the letter, not of the paragraph.
    #[test]
    fn the_box_of_a_drop_cap_is_the_letter() {
        let mut session = under_h2(DROP_CAP);
        let (paragraph, ..) = opening(&session);
        let (_, letter) = run_where(
            &mut session,
            |item| matches!(item, DrawItem::Text { text, .. } if text == "m"),
        );
        let boxes = session
            .inspect(paragraph.pseudo(PseudoElement::FirstLetter))
            .expect("the drop cap")
            .boxes;
        assert_eq!(boxes.len(), 1);
        assert!(
            close(&boxes[0], &letter),
            "{:?} against {letter:?}",
            boxes[0]
        );
        let whole = session.inspect(paragraph).expect("the paragraph").boxes;
        assert!(
            boxes[0].width < whole[0].width / 4.0,
            "{boxes:?} against {whole:?}"
        );
    }

    /// Acceptance: a host maps a glyph of a drop cap to the same
    /// manuscript byte as before.
    #[test]
    fn a_glyph_of_a_drop_cap_maps_to_the_same_manuscript_byte() {
        let first_byte = |css: &str, opens: &str| {
            let mut session = under_h2(css);
            let (_, text, _) = opening(&session);
            let (item, _) = run_where(&mut session, |item| {
                matches!(item, DrawItem::Text { origin: Some(origin), text: run, .. }
                    if origin.node == text && run.starts_with(opens))
            });
            let DrawItem::Text {
                origin: Some(origin),
                source_map,
                glyphs,
                ..
            } = item
            else {
                unreachable!("the run names a node");
            };
            let at = glyphs[0].range.start;
            let at = source_map.get(at as usize).copied().unwrap_or(at);
            (origin.node, origin.range.start + at)
        };
        let plain = first_byte("", "my father");
        assert_eq!(first_byte(DROP_CAP, "m"), plain);
        assert_eq!(plain.1, 0, "the paragraph opens at its first byte");
    }

    /// Acceptance: a glyph of a drop cap that opens with punctuation
    /// maps to the manuscript byte of that punctuation.
    #[test]
    fn a_glyph_of_a_quoted_drop_cap_maps_to_the_byte_of_its_punctuation() {
        let mut session = under_h2_opening(DROP_CAP, "\u{201C}");
        let (_, text, _) = opening(&session);
        let (item, _) = run_where(
            &mut session,
            |item| matches!(item, DrawItem::Text { text, .. } if text == "\u{201C}m"),
        );
        let DrawItem::Text {
            origin: Some(origin),
            source_map,
            glyphs,
            ..
        } = item
        else {
            unreachable!("the run names a node");
        };
        assert_eq!(origin.node, text);
        let byte = |glyph: usize| {
            let at = glyphs[glyph].range.start;
            origin.range.start + source_map.get(at as usize).copied().unwrap_or(at)
        };
        assert_eq!(
            byte(0),
            0,
            "the quotation mark is the paragraph's first byte"
        );
        assert_eq!(byte(1), "\u{201C}".len() as u32, "the letter follows it");
    }

    /// Part: a run cut from a pseudo-element carries its id, and its
    /// `origin` still names the manuscript text.
    #[test]
    fn a_run_cut_from_a_pseudo_element_carries_its_id_beside_its_origin() {
        let css = format!("{DROP_CAP} {SMALL_CAPS} {LINK}");
        let mut session = under_h2(&css);
        let (paragraph, text, link) = opening(&session);
        let runs: Vec<(String, Option<NodeId>, Option<NodeId>)> = session
            .preview()
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter_map(|item| match item {
                DrawItem::Text {
                    text,
                    origin,
                    pseudo_element,
                    ..
                } => Some((
                    text.clone(),
                    origin.as_ref().map(|origin| origin.node),
                    *pseudo_element,
                )),
                _ => None,
            })
            .collect();

        let cap = runs.iter().find(|(run, ..)| run == "m").expect("the cap");
        let letter = paragraph.pseudo(PseudoElement::FirstLetter);
        assert_eq!((cap.1, cap.2), (Some(text), Some(letter)));

        let line = paragraph.pseudo(PseudoElement::FirstLine);
        let opening: Vec<_> = runs.iter().filter(|(.., id)| *id == Some(line)).collect();
        assert!(!opening.is_empty(), "no run carries the `::first-line` id");
        assert!(
            opening.iter().all(|(_, origin, _)| *origin == Some(text)),
            "{opening:?}"
        );

        let after = link.pseudo(PseudoElement::After);
        let generated = runs
            .iter()
            .find(|(run, ..)| run.contains("(link)"))
            .expect("the text of `a::after`");
        assert_eq!((generated.1, generated.2), (Some(after), Some(after)));
    }

    /// Part: the inspection names both the element and the
    /// pseudo-element.
    #[test]
    fn an_inspection_names_the_element_and_the_pseudo_element() {
        let mut session = under_h2(DROP_CAP);
        let (paragraph, ..) = opening(&session);
        let letter = paragraph.pseudo(PseudoElement::FirstLetter);
        let inspection = session.inspect(letter).expect("the drop cap");
        assert_eq!(inspection.node, Some(letter));
        assert_eq!(inspection.element, "p");
        assert_eq!(inspection.pseudo_element.as_deref(), Some("::first-letter"));
        let json = serde_json::to_value(&inspection).expect("serializes");
        assert_eq!(json["pseudoElement"], "::first-letter");

        let plain = session.inspect(paragraph).expect("the paragraph");
        assert_eq!(plain.pseudo_element, None);
        assert!(!author_selectors(&plain).contains(&"h2 + p::first-letter"));
        let json = serde_json::to_value(&plain).expect("serializes");
        assert!(json.get("pseudoElement").is_none());

        assert_eq!(
            session.inspect(paragraph.pseudo(PseudoElement::FirstLine)),
            None,
            "no rule styles `::first-line`"
        );
    }

    /// Acceptance: with `h2 + p::first-letter { initial-letter: 3 }`,
    /// the inspection of the paragraph's `::first-letter` names the
    /// paragraph's id, and inspecting that id answers for the
    /// paragraph.
    #[test]
    fn a_pseudo_element_names_the_element_it_belongs_to() {
        let mut session = under_h2(DROP_CAP);
        let (paragraph, ..) = opening(&session);
        let letter = paragraph.pseudo(PseudoElement::FirstLetter);
        let inspection = session.inspect(letter).expect("the drop cap");
        assert_eq!(inspection.node, Some(letter));
        assert_eq!(inspection.element_node, Some(paragraph));

        let element = inspection.element_node.expect("the element it belongs to");
        let inspection = session.inspect(element).expect("the paragraph");
        assert_eq!(inspection.node, Some(paragraph));
        assert_eq!(inspection.element, "p");
        assert_eq!(inspection.pseudo_element, None);
    }

    /// Acceptance: the inspection of an element names the element
    /// itself, the same id as `node`.
    #[test]
    fn an_element_names_itself() {
        let mut session = under_h2(DROP_CAP);
        let (paragraph, text, _) = opening(&session);
        let inspection = session.inspect(paragraph).expect("the paragraph");
        assert_eq!(inspection.element_node, inspection.node);
        assert_eq!(inspection.element_node, Some(paragraph));

        let held = session.inspect(text).expect("the text it holds");
        assert_eq!(held.element_node, Some(paragraph));
    }

    /// Acceptance: the inspection of a margin box names no element.
    #[test]
    fn a_margin_box_names_no_element() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![section("one.md", prose("alpha", 12))]));
        session.set_style(sheets("@page :left { @top-left { content: \"Left\" } }"));
        let index = session
            .preview()
            .pages
            .iter()
            .position(|page| page.side == Side::Verso)
            .expect("a left page");
        let inspection = session
            .inspect_margin_box(index, MarginBox::TopLeft)
            .expect("the left page names @top-left");
        assert_eq!(inspection.element, "@top-left");
        assert_eq!(inspection.element_node, None);
    }

    /// Acceptance: the JSON the wasm binding returns carries the
    /// element.
    #[test]
    fn the_json_of_an_inspection_carries_the_element() {
        let mut session = under_h2(DROP_CAP);
        let (paragraph, ..) = opening(&session);
        let letter = paragraph.pseudo(PseudoElement::FirstLetter);
        let drop_cap = session.inspect(letter).expect("the drop cap");
        let json = serde_json::to_value(&drop_cap).expect("serializes");
        assert_eq!(json["elementNode"], paragraph.get());

        let plain = session.inspect(paragraph).expect("the paragraph");
        let json = serde_json::to_value(&plain).expect("serializes");
        assert_eq!(json["elementNode"], paragraph.get());
    }

    /// Acceptance: a stylesheet edit that adds or removes a
    /// pseudo-element leaves the id of every content node unchanged.
    #[test]
    fn a_sheet_that_adds_or_removes_a_pseudo_element_keeps_every_node_id() {
        let mut session = chapter("", emphatic());
        let ids = |session: &mut Session<'_>| -> Vec<(u32, Option<std::ops::Range<u32>>)> {
            session.preview();
            let book = session.book();
            let whole = book
                .subtree(book.sections[0].id)
                .expect("the chapter holds its nodes");
            (whole.start..whole.end)
                .map(|id| (id, book.subtree(NodeId::new(id))))
                .collect()
        };
        let plain = ids(&mut session);
        session.set_style(sheets(PSEUDO_ELEMENTS));
        assert_eq!(
            ids(&mut session),
            plain,
            "adding pseudo-elements renumbered"
        );
        session.set_style(sheets(""));
        assert_eq!(ids(&mut session), plain, "removing them renumbered");
    }

    /// Every pseudo-element, on blocks and on an inline element.
    const PSEUDO_ELEMENTS: &str = "p::before { content: \"x\" } \
        section::after { content: \"\"; height: 2pt } \
        p::first-letter { initial-letter: 2 } \
        p::first-line { font-variant-caps: small-caps } \
        em::before { content: \"[\" } em::after { content: \"]\" }";

    /// Acceptance: two layouts of a book with pseudo-elements are
    /// byte-identical.
    #[test]
    fn two_layouts_with_pseudo_elements_are_byte_identical() {
        let css = format!(
            "{PSEUDO_ELEMENTS} p:first-child::before {{ content: \"\\201C\"; \
             position: absolute; top: 0; left: 0; wrap-flow: end }} \
             p::after {{ content: \"\"; height: 2pt; background-color: #eeddcc }}"
        );
        let bytes = || crate::wire::encode(chapter(&css, emphatic()).preview()).expect("encodes");
        let first = bytes();
        assert_eq!(first, bytes());
        let pseudo = crate::wire::decode(&first)
            .expect("decodes")
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter(|item| {
                matches!(
                    item,
                    DrawItem::Text {
                        pseudo_element: Some(_),
                        ..
                    }
                )
            })
            .count();
        assert!(pseudo > 0, "no run carries a pseudo-element");
    }

    /// A session over a book read from JSON, under `css`.
    fn from_json(blocks: &str, css: &str) -> Session<'static> {
        let json = format!(r#"{{"metadata": {{}}, "sections": [{{"blocks": [{blocks}]}}]}}"#);
        let book: crate::content::Book = serde_json::from_str(&json).expect("the book reads");
        let mut session = Session::new(registry());
        session.set_content(book);
        session.set_style(sheets(css));
        session
    }

    fn holds(outer: &PageBox, inner: &PageBox) -> bool {
        let slack = 0.01;
        outer.page == inner.page
            && inner.x >= outer.x - slack
            && inner.y >= outer.y - slack
            && inner.x + inner.width <= outer.x + outer.width + slack
            && inner.y + inner.height <= outer.y + outer.height + slack
    }

    #[test]
    fn a_table_row_its_cells_and_their_blocks_answer_with_their_boxes() {
        let cell = |text: &str| {
            format!(
                r#"{{"cells": [{{"blocks": [{{"type": "paragraph",
                    "inlines": [{{"type": "text", "value": "{text}"}}]}}]}}]}}"#
            )
        };
        let table = format!(
            r#"{{"type": "table", "head": [{}], "body": [{}]}}"#,
            cell("Name"),
            cell("Gulliver")
        );
        let mut session = from_json(&table, "td p { background-color: #eeddcc; padding: 6pt }");
        let Block::Table { body, .. } = &session.book().sections[0].blocks[0] else {
            panic!("a table");
        };
        let (row, data) = (body[0].id, body[0].cells[0].id);
        let paragraph = block_id(&body[0].cells[0].blocks[0]);
        let painted = tinted(&mut session);

        let inner = session.inspect(paragraph).expect("the paragraph").boxes;
        assert_eq!(inner.len(), 1);
        assert!(
            close(&inner[0], &painted[0]),
            "{:?} against {:?}",
            inner[0],
            painted[0]
        );
        let cell_box = session.inspect(data).expect("the cell").boxes;
        assert_eq!(cell_box.len(), 1);
        assert!(
            holds(&cell_box[0], &inner[0]),
            "the cell holds its paragraph"
        );
        let row_box = session.inspect(row).expect("the row").boxes;
        assert_eq!(row_box.len(), 1);
        assert!(holds(&row_box[0], &cell_box[0]), "the row holds its cell");

        assert_eq!(
            session.hit(0, inner[0].x + 2.0, inner[0].y + 2.0),
            Some(paragraph),
            "the padding of a paragraph in a cell is the paragraph's"
        );
    }

    #[test]
    fn a_block_against_the_page_answers_with_its_box_and_wins_a_point_it_covers() {
        let quote = r#"{"type": "blockquote", "blocks": [{"type": "paragraph",
            "inlines": [{"type": "text", "value": "Motto"}]}]}"#;
        let prose: Vec<String> = (0..3)
            .map(|_| {
                format!(
                    r#"{{"type": "paragraph", "inlines": [{{"type": "text", "value": "{}"}}]}}"#,
                    "alpha ".repeat(80)
                )
            })
            .collect();
        let blocks = format!("{quote}, {}", prose.join(", "));
        let css = "blockquote { position: absolute; top: 0; left: 0; right: 150pt; \
                   wrap-flow: both; background-color: #eeddcc; padding: 12pt }";
        let mut session = from_json(&blocks, css);
        let quote = block_id(&session.book().sections[0].blocks[0]);
        let painted = tinted(&mut session);

        let area = session.inspect(quote).expect("the quote").boxes;
        assert_eq!(area.len(), 1);
        assert_eq!(painted.len(), 1);
        assert!(
            close(&area[0], &painted[0]),
            "{:?} against {:?}",
            area[0],
            painted[0]
        );
        let motto = NodeId::new(quote.get() + 1);
        let inner = session.inspect(motto).expect("the motto").boxes;
        assert_eq!(inner.len(), 1);
        assert!(holds(&area[0], &inner[0]), "the quote holds its paragraph");

        assert_eq!(
            session.hit(0, area[0].x + 3.0, area[0].y + 3.0),
            Some(quote),
            "the padding of the quote is the quote's, over the chapter under it"
        );
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
