//! The stages themselves, and how far down an edit reaches.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};

use crate::Warning;
use crate::layout::{Named, Paged, Paginator, References, landed, moved};

use super::invalidate::{Against, Prints, hyphenation, section_local};
use super::key::{section_key, settled_key};
use super::output::blank_output;
use super::{Cached, Session, Stale};

impl Session<'_> {
    /// Compiles the styling again and classifies what moved.
    pub(super) fn recompile(&mut self) {
        let Some(sheets) = &self.sheets else {
            return;
        };
        let styles = sheets.compile(&self.book, self.registry.get());
        self.stages.style += 1;
        let prints = Prints::of(&styles, self.images);
        self.stale = self.stale.max(self.prints.against(&prints));
        self.prints = prints;
        self.section_local = section_local(&styles);
        self.styles = Cow::Owned(styles);
    }

    /// Runs the stages the last change invalidated, and no others.
    pub(super) fn update(&mut self) {
        if self.stale >= Stale::Trace {
            self.trace();
        }
        if self.retain {
            if self.stale >= Stale::Break {
                self.rebreak();
            }
            if self.stale >= Stale::Flow {
                self.reflow();
            }
            if self.stale >= Stale::Paint {
                self.repaint();
            }
        } else if self.stale != Stale::Nothing {
            self.run_once();
        }
        self.stale = Stale::Nothing;
        self.collect_warnings();
    }

    /// Traces the contours the cascade asks for, keeping the ones an
    /// earlier run already traced.
    fn trace(&mut self) {
        self.stages.trace += self
            .contours
            .update(&self.book, &self.styles, self.assets.get());
    }

    /// Breaks the sections whose lines the cache cannot answer for,
    /// and keeps the rest as they stand.
    fn rebreak(&mut self) {
        let against = Against::of(&self.styles, self.images);
        // An image that arrives is a box that was not reserved
        // before it, so the sections are broken again around it. The
        // table only grows, so a count that moved is an image.
        let supplied = self.images.then(|| self.assets.get().assets().len());
        self.references = if self.styles.refers() {
            References::of(&self.book)
        } else {
            References::default()
        };
        let mut previous: Vec<Option<Cached>> = std::mem::take(&mut self.lines)
            .into_iter()
            .map(Some)
            .collect();
        let mut spare: HashMap<u64, Vec<usize>> = HashMap::new();
        if self.section_local {
            for (index, cached) in previous.iter().enumerate() {
                let key = cached.as_ref().expect("nothing is taken yet").key;
                spare.entry(key).or_default().push(index);
            }
        }
        let mut fresh = Vec::with_capacity(self.book.sections.len());
        for section in &self.book.sections {
            let key = section_key(
                section,
                &self.styles,
                against,
                supplied,
                hyphenation(&self.book.metadata),
                &self.references,
            );
            let kept = spare
                .get_mut(&key)
                .and_then(|slots| slots.pop())
                .and_then(|slot| previous[slot].take());
            fresh.push(match kept {
                Some(mut cached) => {
                    cached.renumber(section.id);
                    cached
                }
                None => {
                    // One paginator per section, so the warnings it
                    // collects are the ones this section raised.
                    let paginator = Paginator::with_contours(
                        self.registry.get(),
                        &self.styles,
                        self.assets.get(),
                        &self.contours,
                    );
                    paginator.language(&self.book.metadata);
                    paginator.refer(self.references.clone());
                    let fragments = paginator.section_fragments(section);
                    self.stages.lines += 1;
                    Cached {
                        key,
                        section: section.id,
                        fragments,
                        warnings: paginator.warnings(),
                    }
                }
            });
        }
        self.lines = fresh;
        self.flow_warnings.clear();
        for cached in &self.lines {
            for warning in &cached.warnings {
                if !self
                    .flow_warnings
                    .iter()
                    .any(|seen| seen.message == warning.message)
                {
                    self.flow_warnings.push(warning.clone());
                }
            }
        }
    }

    /// Flows the cached lines into pages. Nothing here measures, but
    /// for the sections whose references print a page, which are
    /// built again once the pages are known.
    fn reflow(&mut self) {
        let mut paged = self.fragment(false);
        if self.styles.counts_pages() {
            paged = self.settle(&paged);
        } else {
            self.settled.clear();
            self.settle_warnings.clear();
        }
        self.infos = paged.infos;
        let (registry, assets) = (self.registry.get(), self.assets.get());
        self.output
            .get_or_insert_with(|| blank_output(registry, assets))
            .pages = paged.pages;
    }

    /// Fragments the book over each section's lines: the lines of the
    /// pass that finds the pages, or, where `settled` asks for them
    /// and a section has them, the lines of the pass that prints them.
    fn fragment(&mut self, settled: bool) -> Paged {
        let paginator = Paginator::with_contours(
            self.registry.get(),
            &self.styles,
            self.assets.get(),
            &self.contours,
        );
        let sections = self.lines.iter().enumerate().map(|(index, cached)| {
            let kept = settled
                .then(|| self.settled.get(index).and_then(Option::as_ref))
                .flatten();
            kept.unwrap_or(cached).fragments.as_slice()
        });
        let paged = paginator.fragment(&self.book, sections);
        self.stages.flow += 1;
        paged
    }

    /// Lays the book out again with the folio each reference prints,
    /// read off the pages of the pass before. Only the sections whose
    /// references print a page are built again, and one that prints
    /// the folios it printed last time keeps the lines it had.
    fn settle(&mut self, paged: &Paged) -> Paged {
        let found = landed(paged);
        let resolved = self.references.landed(found.clone());
        let mut spare: HashMap<u64, Vec<Cached>> = HashMap::new();
        for cached in std::mem::take(&mut self.settled).into_iter().flatten() {
            if self.section_local {
                spare.entry(cached.key).or_default().push(cached);
            }
        }
        let mut printed = BTreeSet::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut settled = Vec::with_capacity(self.book.sections.len());
        for (section, first) in self.book.sections.iter().zip(&self.lines) {
            let named = Named::in_section(section, &self.styles);
            if named.pages.is_empty() {
                settled.push(None);
                continue;
            }
            let key = settled_key(first.key, &named.pages, &found);
            printed.extend(named.pages);
            let cached = match spare.get_mut(&key).and_then(Vec::pop) {
                Some(mut cached) => {
                    cached.renumber(section.id);
                    cached
                }
                None => {
                    let paginator = Paginator::with_contours(
                        self.registry.get(),
                        &self.styles,
                        self.assets.get(),
                        &self.contours,
                    );
                    paginator.language(&self.book.metadata);
                    paginator.refer(resolved.clone());
                    let fragments = paginator.section_fragments(section);
                    self.stages.lines += 1;
                    Cached {
                        key,
                        section: section.id,
                        fragments,
                        warnings: paginator.warnings(),
                    }
                }
            };
            for warning in &cached.warnings {
                if !warnings.iter().any(|seen| seen.message == warning.message) {
                    warnings.push(warning.clone());
                }
            }
            settled.push(Some(cached));
        }
        self.settled = settled;
        self.stages.settle += 1;
        let paged = self.fragment(true);
        warnings.extend(moved(&found, &landed(&paged), &printed));
        self.settle_warnings = warnings;
        paged
    }

    /// Repaints the furniture over pages the flow already settled.
    fn repaint(&mut self) {
        let paginator = Paginator::with_contours(
            self.registry.get(),
            &self.styles,
            self.assets.get(),
            &self.contours,
        );
        if let Some(output) = &mut self.output {
            paginator.paint(&mut output.pages, &self.infos);
            self.stages.paint += 1;
        }
    }

    /// The whole pipeline, one section's lines alive at a time.
    fn run_once(&mut self) {
        let registry = self.registry.get();
        let assets = self.assets.get();
        let paginator = Paginator::with_contours(
            self.registry.get(),
            &self.styles,
            self.assets.get(),
            &self.contours,
        );
        let pages = paginator.paginate(&self.book);
        let passes = 1 + paginator.settles();
        self.stages.lines += self.book.sections.len() as u32 * passes;
        self.stages.flow += passes;
        self.stages.settle += paginator.settles();
        self.stages.paint += 1;
        self.flow_warnings = paginator.warnings();
        self.output
            .get_or_insert_with(|| blank_output(registry, assets))
            .pages = pages;
    }

    /// Everything the run has to complain about, in the order the
    /// stages raised it.
    fn collect_warnings(&mut self) {
        let mut warnings = self.source_warnings.clone();
        warnings.extend(self.styles.warnings().iter().cloned());
        warnings.extend(self.assets.get().warnings().iter().cloned());
        warnings.extend(self.contours.warnings().iter().cloned());
        warnings.extend(self.flow_warnings.iter().cloned());
        for warning in &self.settle_warnings {
            if !warnings.iter().any(|seen| seen.message == warning.message) {
                warnings.push(warning.clone());
            }
        }
        if let Some(output) = &mut self.output {
            output.warnings = warnings;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Warning;
    use crate::content::{Attributes, Block, HeadingLevel, Inline, Metadata, NodeId, Section};
    use crate::pages::DrawItem;
    use crate::session::testing::{
        MAP, alpha_png, book, book_with_image, declaring, gif, hyphenated, illustrated, painted,
        prose, runs, section, sheets, three_chapters,
    };
    use crate::session::{Session, Stages};
    use crate::style::Color;

    /// An image the sheet leaves in the flow is not decoded, however
    /// the sheet names its contour. Nothing sets beside such an
    /// image, so a contour on it would answer a question nobody
    /// asks, and the run says nothing about it either.
    #[test]
    fn an_image_left_in_the_flow_is_not_traced() {
        for css in [
            "img { shape-outside: auto }",
            "img { shape-outside: auto; wrap-flow: end }",
            "img { shape-outside: auto; position: absolute; wrap-flow: auto }",
        ] {
            let mut session = illustrated();
            session.set_style(sheets(css));
            let warnings = session.preview().warnings.clone();
            assert_eq!(session.stages().trace, 0, "{css} decoded an image");
            assert!(
                !warnings.iter().any(|w| w.message.contains("alpha")),
                "{css} complained: {warnings:?}",
            );
        }

        // Anchored, and setting prose on one side of it, the same
        // image is traced once.
        let mut session = illustrated();
        session.set_style(sheets(
            "img { shape-outside: auto; position: absolute; wrap-flow: end }",
        ));
        session.preview();
        assert_eq!(session.stages().trace, 1);
    }

    /// Acceptance: a book whose sheet names no contour decodes
    /// nothing, and neither does one that writes its contour out as a
    /// polygon. Only `shape-outside: auto` reaches the pixels.
    #[test]
    fn only_a_traced_contour_decodes_an_image() {
        let anchored = "img { position: absolute; top: 0; left: 0; wrap-flow: end";

        let mut bare = illustrated();
        bare.set_style(sheets(&format!("{anchored} }}")));
        bare.preview();
        assert_eq!(bare.stages().trace, 0, "a sheet with no contour decoded");

        let mut written = illustrated();
        written.set_style(sheets(&format!(
            "{anchored}; shape-outside: polygon(0 0, 100% 0, 100% 100%) }}"
        )));
        written.preview();
        assert_eq!(written.stages().trace, 0, "a polygon decoded");

        let mut traced = illustrated();
        traced.set_style(sheets(&format!("{anchored}; shape-outside: auto }}")));
        traced.preview();
        assert_eq!(traced.stages().trace, 1, "the contour was not traced");
    }

    /// Acceptance: editing the sheet so a plate stops wrapping
    /// re-traces nothing. Neither does an edit that leaves the
    /// contours where they were.
    #[test]
    fn a_sheet_edit_that_leaves_the_contours_re_traces_nothing() {
        let mut session = illustrated();
        let sheet = |rest: &str| {
            sheets(&format!(
                "img {{ position: absolute; top: 0; left: 0; shape-outside: auto; {rest} }}"
            ))
        };
        session.set_style(sheet("wrap-flow: end"));
        session.preview();
        assert_eq!(session.stages().trace, 1);

        // The plate stops wrapping: the contour it traced stands.
        session.set_style(sheet("wrap-flow: auto"));
        session.preview();
        assert_eq!(session.stages().trace, 1, "a plate that stopped wrapping");

        // So does a recolour, which reaches no stage above the paint.
        session.set_style(sheet("wrap-flow: end; color: #333333"));
        session.preview();
        assert_eq!(session.stages().trace, 1, "a recolour re-traced");

        // Different pixels at the same url are a different contour.
        session.add_image("plate.png", alpha_png(0x55)).unwrap();
        session.preview();
        assert_eq!(session.stages().trace, 2, "new pixels were not traced");

        // The same pixels again are not.
        session.add_image("plate.png", alpha_png(0x55)).unwrap();
        session.preview();
        assert_eq!(session.stages().trace, 2);
    }

    /// Acceptance: two runs over a traced contour are byte-identical.
    #[test]
    fn two_runs_over_a_traced_contour_agree() {
        let css = "img { position: absolute; top: 0; left: 0; wrap-flow: end; \
                   shape-outside: auto; shape-margin: 6pt }";
        let wire = || {
            let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
            let mut book = book_with_image("plate.png");
            book.sections[0]
                .blocks
                .extend((0..8).map(|_| Block::Paragraph {
                    id: NodeId::UNASSIGNED,
                    inlines: vec![Inline::Text {
                        id: NodeId::UNASSIGNED,
                        value: "my father had a small estate in nottinghamshire ".repeat(6),
                        attributes: Attributes::default(),
                        position: None,
                        span: None,
                    }],
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                }));
            book.assign_node_ids();
            session.set_content(book);
            session.add_image("plate.png", alpha_png(0x22)).unwrap();
            session.set_style(sheets(css));
            crate::wire::encode(session.preview()).expect("the display structure encodes")
        };
        assert_eq!(wire(), wire());
    }

    /// An image asked to wrap to its own shape that carries no alpha
    /// keeps the prose clear of its box, and the run says so once.
    #[test]
    fn an_image_with_no_alpha_to_trace_contributes_its_box() {
        let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
        session.set_content(book_with_image("pic.gif"));
        session.add_image("pic.gif", gif(64, 32, 0)).unwrap();
        session.set_style(sheets(
            "img { position: absolute; top: 0; left: 0; wrap-flow: end; shape-outside: auto }",
        ));
        let warnings = session.preview().warnings.clone();
        let complaints: Vec<&Warning> = warnings
            .iter()
            .filter(|warning| warning.message.contains("Missing alpha channel"))
            .collect();
        assert_eq!(complaints.len(), 1, "{warnings:?}");
        assert!(complaints[0].message.contains("pic.gif"));

        // Asked again, complained about once.
        session.preview();
        assert_eq!(session.stages().trace, 1);
    }

    /// A session sets the prose around an image the sheet anchors. A
    /// sheet that moves the image builds the narrowed sections again,
    /// because the lines beside an image cannot be kept when the
    /// image moves.
    #[test]
    fn a_sheet_that_moves_an_anchored_image_builds_the_sections_again() {
        let mut book = book(vec![section(
            "one.md",
            [
                vec![Block::Image {
                    id: NodeId::UNASSIGNED,
                    url: "plate.jpg".into(),
                    alt: "a map".into(),
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                }],
                prose("alpha", 6),
            ]
            .concat(),
        )]);
        book.assign_node_ids();
        let mut session =
            Session::owning(crate::fonts::bundled_registry().expect("bundled font parses"));
        session.set_content(book);
        session.add_image("plate.jpg", MAP.to_vec()).unwrap();
        session.set_style(sheets(
            "img { position: absolute; top: 0; left: 0; margin-right: 12pt; wrap-flow: end }",
        ));
        let output = session.preview();
        let image = output
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .find_map(|item| match item {
                DrawItem::Image { x, w, .. } => Some((*x, *w)),
                _ => None,
            })
            .expect("the image is painted");
        let beside = output.pages[0]
            .items
            .iter()
            .filter(|item| matches!(item, DrawItem::Text { x, .. } if *x >= image.0 + image.1))
            .count();
        assert!(beside > 0, "no line is set beside the image");

        let before = session.stages();
        session.set_style(sheets(
            "img { position: absolute; top: 0; right: 0; margin-left: 12pt; wrap-flow: start }",
        ));
        session.preview();
        assert_eq!(
            session.stages().lines,
            before.lines + 1,
            "the section was kept over an image that moved",
        );
    }

    /// Acceptance: a book with nothing anchored to the page breaks
    /// its lines as often as it did before exclusions existed. The
    /// pass that settles where an image lands is a cost only a book
    /// with an image pays.
    #[test]
    fn a_book_with_nothing_anchored_breaks_its_lines_as_often_as_ever() {
        let mut session = three_chapters();
        let before = session.stages();
        assert_eq!(before.lines, 3, "one break per section, once");

        // This sheet says something about anchoring, but the book
        // has no image to anchor. Nothing the sheet says reaches a
        // node, so no stage runs again.
        session.set_style(sheets("img { position: absolute; wrap-flow: end }"));
        session.preview();
        assert_eq!(
            session.stages(),
            Stages {
                style: before.style + 1,
                ..before
            },
            "a sheet that reaches no node reached a stage",
        );

        // A page box that moves still re-fragments over the lines it
        // has.
        session.set_style(sheets("@page { margin-bottom: 108pt }"));
        session.preview();
        let after = session.stages();
        assert_eq!(after.lines, before.lines, "the lines were broken again");
        assert_eq!(after.flow, before.flow + 1, "fragmentation did not run");
    }

    /// Naming a book costs nothing: a rename that leaves the
    /// hyphenation where it was reaches no stage between the content
    /// tree and the page, so the pages already laid out are the ones
    /// the export writes under the new name.
    #[test]
    fn naming_a_book_re_runs_no_stage() {
        let mut session = three_chapters();
        let before = session.stages();
        let pages = session.preview().pages.len();
        session.set_metadata(Metadata {
            title: Some("Gulliver's Travels".into()),
            author: Some("Jonathan Swift".into()),
            extra: [("language".to_string(), "en".to_string())]
                .into_iter()
                .collect(),
        });
        let after = session.preview().pages.len();
        assert_eq!(session.stages(), before);
        assert_eq!(after, pages);
        assert_eq!(
            session.book().metadata.title.as_deref(),
            Some("Gulliver's Travels")
        );
    }

    /// A language that chooses other patterns breaks the lines
    /// again: the ones the session kept were broken by the patterns
    /// the old tag chose.
    #[test]
    fn a_new_language_re_breaks_the_lines() {
        let mut session = hyphenated(Some("en"), "Wassermann");
        let english = painted(session.preview());
        let before = session.stages();

        session.set_metadata(declaring("de"));
        let german = painted(session.preview());

        assert!(
            session.stages().lines > before.lines,
            "the lines were not broken again"
        );
        assert_eq!(english, ["Wasser-", "mann"]);
        assert_eq!(german, ["Was-", "ser-", "mann"]);
    }

    /// A tag with no patterns replaced by another warns about the
    /// new one, though both leave every word whole.
    #[test]
    fn a_new_unknown_language_warns_about_itself() {
        let mut session = hyphenated(Some("xx"), "Wassermann");
        session.preview();
        session.set_metadata(declaring("yy"));
        let warnings = &session.preview().warnings;
        assert!(
            warnings.iter().any(|w| w.message.contains("yy")),
            "warnings: {warnings:?}"
        );
        assert!(
            !warnings.iter().any(|w| w.message.contains("xx")),
            "the old tag is still complained about: {warnings:?}"
        );
    }

    /// A `@page` change that leaves the measure where it was is
    /// answered by fragmentation: the lines it flows are the ones it
    /// already had.
    #[test]
    fn page_geometry_re_fragments_without_breaking_lines() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_style(sheets("@page { margin-bottom: 108pt }"));
        session.preview();
        let after = session.stages();
        assert_eq!(after.lines, before.lines, "the lines were broken again");
        assert_eq!(after.flow, before.flow + 1, "fragmentation did not run");
        assert_eq!(
            after.paint,
            before.paint + 1,
            "the furniture was not painted"
        );
    }

    /// The margin boxes are shallower still: the pages are settled,
    /// and only their furniture is painted again.
    #[test]
    fn a_running_foot_repaints_over_settled_pages() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_style(sheets("@page { @bottom-center { content: \"leaf\" } }"));
        let footed = !runs(session.preview(), "leaf").is_empty();
        let after = session.stages();
        assert!(footed, "the new foot was never painted");
        assert_eq!(after.lines, before.lines, "the lines were broken again");
        assert_eq!(after.flow, before.flow, "the pages were fragmented again");
        assert_eq!(
            after.paint,
            before.paint + 1,
            "the furniture was not painted"
        );
    }

    /// A page repainted twice ends up with one set of furniture,
    /// because the paint discards what the last one left rather than
    /// stacking on it.
    #[test]
    fn repainting_does_not_stack_furniture() {
        let mut session = three_chapters();
        let once = session.preview().pages[1].items.len();
        session.set_style(sheets(
            "@page { @bottom-center { content: counter(page) } }",
        ));
        let twice = session.preview().pages[1].items.len();
        assert_eq!(once, twice, "the second folio was painted over the first");
    }

    /// A change the engine models nothing of costs nothing. The
    /// display structure is served back as it stands, and only the
    /// diagnostics move.
    #[test]
    fn an_unsupported_property_runs_no_stage() {
        let mut session = three_chapters();
        let before = session.stages();
        let painted = serde_json::to_vec(&session.preview().pages).expect("pages serialize");

        session.set_style(sheets("p { float: left }"));
        let (repainted, complained) = {
            let output = session.preview();
            (
                serde_json::to_vec(&output.pages).expect("pages serialize"),
                output
                    .warnings
                    .iter()
                    .any(|warning| warning.message.contains("`float`")),
            )
        };
        let after = session.stages();

        assert_eq!(repainted, painted, "the display structure changed");
        assert!(complained, "the unsupported property went unreported");
        assert_eq!(after.lines, before.lines, "the lines were broken again");
        assert_eq!(after.flow, before.flow, "the pages were fragmented again");
        assert_eq!(after.paint, before.paint, "the furniture was painted again");
    }

    const PAGE_REFERENCE: &str =
        "a::after { content: \" (page \" target-counter(attr(href url), page) \")\" }";

    /// A chapter read from `source` that opens on a heading carrying
    /// `id`, whose first paragraph links to `to`, over eight
    /// paragraphs of prose tagged `tag`.
    fn linked_chapter(source: &str, title: &str, id: &str, to: &str, tag: &str) -> Section {
        let words = |value: &str| Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        };
        let mut blocks = vec![
            Block::Heading {
                id: NodeId::UNASSIGNED,
                level: HeadingLevel::H1,
                inlines: vec![words(title)],
                attributes: Attributes {
                    id: Some(id.into()),
                    classes: Vec::new(),
                },
                position: None,
                span: None,
            },
            Block::Paragraph {
                id: NodeId::UNASSIGNED,
                inlines: vec![
                    words("See "),
                    Inline::Link {
                        id: NodeId::UNASSIGNED,
                        url: to.into(),
                        children: vec![words("there")],
                        attributes: Attributes::default(),
                        position: None,
                        span: None,
                    },
                    words("."),
                ],
                attributes: Attributes::default(),
                position: None,
                span: None,
            },
        ];
        blocks.extend(prose(tag, 8));
        section(source, blocks)
    }

    /// Three chapters, one file each. The first links to the heading
    /// the third opens on, the third links back to the first, and the
    /// second links to nothing.
    fn referring() -> Session<'static> {
        let mut session = Session::new(crate::session::testing::registry());
        session.set_content(book(vec![
            linked_chapter("one.md", "The Voyage", "the-voyage", "#the-hunter", "alpha"),
            section("two.md", prose("beta", 8)),
            linked_chapter(
                "three.md",
                "The Hunter",
                "the-hunter",
                "#the-voyage",
                "gamma",
            ),
        ]));
        session.set_style(sheets(PAGE_REFERENCE));
        session.preview();
        session
    }

    /// The folio the heading that opens one section is set on.
    fn heading_folio(session: &mut Session<'_>, section: usize) -> u32 {
        let node = crate::content::block_id(&session.book().sections[section].blocks[0]);
        session.folios(&[node])[0]
            .expect("the heading is set")
            .first
    }

    /// Every page number the book's references print, in reading
    /// order.
    fn printed(output: &crate::LayoutOutput) -> Vec<u32> {
        let whole = painted(output).join(" ");
        whole
            .split("(page ")
            .skip(1)
            .map(|rest| {
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().expect("a number follows every reference")
            })
            .collect()
    }

    /// Acceptance: a book with no reference to a page lays out in one
    /// pass. The words of the element a link names are in the book
    /// before anything is laid out, so a sheet that prints them costs
    /// no second pass either.
    #[test]
    fn a_book_with_no_page_reference_lays_out_in_one_pass() {
        let mut session = three_chapters();
        session.set_style(sheets(PAGE_REFERENCE));
        session.preview();
        let stages = session.stages();
        assert_eq!(stages.settle, 0, "a book with no link settled");
        assert_eq!(stages.flow, 1);

        let mut session = referring();
        let before = session.stages();
        session.set_style(sheets(
            "a::after { content: \" (\" target-text(attr(href url)) \")\" }",
        ));
        session.preview();
        let after = session.stages();
        assert_eq!(after.settle, before.settle, "printing words settled");
        assert_eq!(after.flow, before.flow + 1);
    }

    /// The second pass builds again only the sections whose references
    /// print a page, and what they print is the folio each heading is
    /// set on.
    #[test]
    fn the_second_pass_builds_only_the_sections_that_print_a_page() {
        let mut session = referring();
        let stages = session.stages();
        assert_eq!(stages.settle, 1);
        assert_eq!(stages.flow, 2);
        assert_eq!(
            stages.lines, 5,
            "every section once, and the two that print a page once more",
        );
        let (voyage, hunter) = (
            heading_folio(&mut session, 0),
            heading_folio(&mut session, 2),
        );
        assert!(hunter > voyage);
        assert_eq!(printed(session.preview()), [hunter, voyage]);
        assert!(session.preview().warnings.is_empty());
    }

    /// A chapter that grows moves the chapter after it. The pass that
    /// finds the pages builds the chapter that grew, and the pass that
    /// prints them builds the chapter whose reference names the one
    /// that moved. The chapter whose reference names a page that did
    /// not move keeps its lines.
    #[test]
    fn a_target_that_moves_re_breaks_only_the_references_to_it() {
        let mut session = referring();
        let hunter = heading_folio(&mut session, 2);
        let before = session.stages();

        session.replace_source("two.md", vec![section("two.md", prose("beta", 24))]);
        session.preview();
        let after = session.stages();
        let moved = heading_folio(&mut session, 2);
        assert!(moved > hunter, "the chapter after the one that grew moved");
        assert_eq!(
            after.lines,
            before.lines + 2,
            "the chapter that grew, and the reference to the one that moved",
        );
        assert_eq!(after.settle, before.settle + 1);
        let voyage = heading_folio(&mut session, 0);
        assert_eq!(printed(session.preview()), [moved, voyage]);
    }

    /// The preview prints the numbers a single run over the same book
    /// prints, on the same pages.
    #[test]
    fn the_preview_prints_what_a_single_run_prints() {
        let mut session = referring();
        session.replace_source("two.md", vec![section("two.md", prose("beta", 24))]);
        let preview = serde_json::to_vec(&session.preview().pages).expect("pages serialize");
        let styles = session.styles().clone();
        let once = crate::layout::layout_book(
            session.book(),
            &styles,
            crate::session::testing::registry(),
            crate::layout::no_assets(),
        );
        assert_eq!(
            serde_json::to_vec(&once.pages).expect("pages serialize"),
            preview,
        );
    }

    /// The words of a heading are part of every section that prints
    /// them. A heading that changes its words builds its own section
    /// again, and the section whose reference prints them.
    #[test]
    fn a_heading_that_changes_its_words_re_breaks_the_references_to_it() {
        let mut session = referring();
        session.set_style(sheets(
            "a::after { content: \" (\" target-text(attr(href url)) \")\" }",
        ));
        session.preview();
        let before = session.stages();

        session.replace_source(
            "three.md",
            vec![linked_chapter(
                "three.md",
                "The Huntress",
                "the-hunter",
                "#the-voyage",
                "gamma",
            )],
        );
        let words = painted(session.preview()).join(" ");
        assert!(words.contains("(The Huntress)"), "{words}");
        assert_eq!(session.stages().lines, before.lines + 2);
    }

    /// A colour edit breaks the lines again: the runs the broken
    /// lines hold are what carry the colour.
    #[test]
    fn a_colour_edit_breaks_the_lines_again() {
        let mut session = three_chapters();
        let before = session.stages();
        session.preview();

        session.set_style(sheets("p { color: rebeccapurple }"));
        let coloured = session
            .preview()
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .any(|item| {
                matches!(item, DrawItem::Text { color, .. } if *color == Color::rgb(102, 51, 153))
            });
        assert!(coloured, "the colour did not reach the page");
        assert!(
            session.stages().lines > before.lines,
            "the lines were served from the break cache in the old colour"
        );
    }
}
