//! The stages themselves, and how far down an edit reaches.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::layout::Paginator;

use super::invalidate::{Against, Prints, hyphenation, section_local};
use super::key::section_key;
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
    pub(super) fn trace(&mut self) {
        self.stages.trace += self
            .contours
            .update(&self.book, &self.styles, self.assets.get());
    }

    /// Breaks the sections whose lines the cache cannot answer for,
    /// and keeps the rest as they stand.
    pub(super) fn rebreak(&mut self) {
        let against = Against::of(&self.styles, self.images);
        // An image that arrives is a box that was not reserved
        // before it, so the sections are broken again around it. The
        // table only grows, so a count that moved is an image.
        let supplied = self.images.then(|| self.assets.get().assets().len());
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

    /// Flows the cached lines into pages. Nothing here measures.
    pub(super) fn reflow(&mut self) {
        let registry = self.registry.get();
        let assets = self.assets.get();
        let paginator = Paginator::with_contours(
            self.registry.get(),
            &self.styles,
            self.assets.get(),
            &self.contours,
        );
        let paged = paginator.fragment(
            &self.book,
            self.lines.iter().map(|cached| cached.fragments.as_slice()),
        );
        self.stages.flow += 1;
        self.infos = paged.infos;
        self.output
            .get_or_insert_with(|| blank_output(registry, assets))
            .pages = paged.pages;
    }

    /// Repaints the furniture over pages the flow already settled.
    pub(super) fn repaint(&mut self) {
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
    pub(super) fn run_once(&mut self) {
        let registry = self.registry.get();
        let assets = self.assets.get();
        let paginator = Paginator::with_contours(
            self.registry.get(),
            &self.styles,
            self.assets.get(),
            &self.contours,
        );
        let pages = paginator.paginate(&self.book);
        self.stages.lines += self.book.sections.len() as u32;
        self.stages.flow += 1;
        self.stages.paint += 1;
        self.flow_warnings = paginator.warnings();
        self.output
            .get_or_insert_with(|| blank_output(registry, assets))
            .pages = pages;
    }

    /// Everything the run has to complain about, in the order the
    /// stages raised it.
    pub(super) fn collect_warnings(&mut self) {
        let mut warnings = self.source_warnings.clone();
        warnings.extend(self.styles.warnings().iter().cloned());
        warnings.extend(self.assets.get().warnings().iter().cloned());
        warnings.extend(self.contours.warnings().iter().cloned());
        warnings.extend(self.flow_warnings.iter().cloned());
        if let Some(output) = &mut self.output {
            output.warnings = warnings;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Warning;
    use crate::content::{Attributes, Block, Inline, Metadata, NodeId};
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
            .filter(|warning| warning.message.contains("no alpha channel"))
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
