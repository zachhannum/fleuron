//! What a host changes: the book, a source, the sheet, a face, an
//! image, the metadata.

use std::borrow::Cow;

use crate::Warning;
use crate::content::{Attributes, Book, Metadata, Section};
use crate::fonts::FontSource;
use crate::images::Added;
use crate::style::Stylesheets;

use super::invalidate::{has_images, hyphenation};
use super::{AddFontError, AddImageError, Session, Stale};

impl Session<'_> {
    /// Sets the book, and with it every stage below box
    /// construction.
    ///
    /// Node identity is the engine's: the tree is renumbered on the
    /// way in, so a host may hand over sections it built by hand.
    pub fn set_content(&mut self, mut book: Book) {
        book.assign_node_ids();
        self.order.clear();
        for section in &book.sections {
            if let Some(source) = &section.source
                && !self.order.contains(source)
            {
                self.order.push(source.clone());
            }
        }
        self.book = Cow::Owned(book);
        self.images = has_images(&self.book);
        self.recompile();
        self.stale = Stale::Trace;
    }

    /// Names the sources of the book in the order they come, one that
    /// has no section among them.
    ///
    /// The book alone cannot say where such a source belongs, and
    /// this is what [`replace_source`](Session::replace_source) places
    /// its sections by once it has some.
    /// [`set_content`](Session::set_content) resets the order to the
    /// sources the book carries, so this follows it.
    pub fn set_source_order(&mut self, names: Vec<String>) {
        self.order = names;
    }

    /// Replaces every section that came from one source file.
    ///
    /// The source is the replaceable unit because a host names files
    /// and one file may split into several sections. A source keeps
    /// its place when it is replaced with no sections, and is set
    /// there again when it is next given some. A name the book has
    /// not seen appends instead, which is how a new file arrives.
    pub fn replace_source(&mut self, name: &str, sections: Vec<Section>) {
        let place = self.order.iter().position(|source| source == name);
        if place.is_none() {
            self.order.push(name.to_string());
        }
        let order = &self.order;
        let book = self.book.to_mut();
        let own = |section: &Section| section.source.as_deref() == Some(name);
        let follows = |section: &Section| {
            let at = section
                .source
                .as_ref()
                .and_then(|source| order.iter().position(|known| known == source));
            matches!((at, place), (Some(at), Some(place)) if at > place)
        };
        let at = book
            .sections
            .iter()
            .position(own)
            .or_else(|| book.sections.iter().position(follows))
            .unwrap_or(book.sections.len());
        // Nothing above the first of its sections is its own, so the
        // index outlives the rest of them going.
        book.sections.retain(|section| !own(section));
        book.sections.splice(at..at, sections);
        book.assign_node_ids();
        self.images = has_images(&self.book);
        self.recompile();
        self.stale = Stale::Trace;
    }

    /// Drops every section that came from one source file, and its
    /// place in the book with them.
    pub fn remove_source(&mut self, name: &str) {
        self.replace_source(name, Vec::new());
        self.order.retain(|source| source != name);
    }

    /// Names every section that came from one source by these classes
    /// and this id, and styles the book again.
    ///
    /// The names sit beside the source rather than in it, so every
    /// node keeps the bytes it was read from. A name the book does
    /// not carry changes nothing.
    pub fn set_source_attributes(&mut self, name: &str, attributes: &Attributes) {
        let book = self.book.to_mut();
        for section in &mut book.sections {
            if section.source.as_deref() == Some(name) {
                section.attributes = attributes.clone();
            }
        }
        self.recompile();
    }

    /// Adds a frontend's complaints to the run's diagnostics.
    ///
    /// A construct the content vocabulary cannot express is reported
    /// where the source was read, which is upstream of every stage a
    /// session runs. The output's warnings are the whole run's, so
    /// they belong in the same channel rather than in a second one
    /// the host has to remember to read. Which of them still apply
    /// after an edit is the caller's to decide; this replaces the
    /// lot.
    pub fn set_source_warnings(&mut self, warnings: Vec<Warning>) {
        self.source_warnings = warnings;
    }

    /// Sets the styling, and with it whichever stage the change
    /// reaches, which is usually far short of everything.
    pub fn set_style(&mut self, sheets: Stylesheets) {
        self.sheets = Some(sheets);
        self.load_faces();
        self.recompile();
    }

    /// Names the book: title, author, and whatever else a frontend
    /// read.
    ///
    /// The declared language is the one field a stage below reads,
    /// and a book that changes it is hyphenated again. The pages
    /// already laid out are otherwise the pages the export writes
    /// under the new name.
    pub fn set_metadata(&mut self, metadata: Metadata) {
        if hyphenation(&metadata) != hyphenation(&self.book.metadata) {
            self.stale = self.stale.max(Stale::Break);
        }
        self.book.to_mut().metadata = metadata;
    }

    /// Registers a face, and re-runs everything a face can change.
    ///
    /// A family the registry did not have is a family the cascade
    /// resolved to something else, so the styling is compiled again
    /// and the lines are broken again. The output's font table is
    /// rebuilt with them.
    ///
    /// Only a session that owns its registry has one to add to; one
    /// that borrowed it says so instead.
    pub fn add_font(&mut self, source: FontSource) -> Result<Vec<u16>, AddFontError> {
        let added = self.register_font(source);
        // The table is built with the output and never patched, so
        // the output goes rather than outlive the ids it indexes.
        self.output = None;
        self.stale = Stale::Break;
        self.recompile();
        added
    }

    /// Registers one image, and the index `DrawItem::Image.asset`
    /// gets for it. `None` for bytes no probe recognises,
    /// which is a diagnostic on the next display structure and no asset.
    ///
    /// A url registered again with the bytes it already answers for
    /// costs nothing. Registered again with different bytes, it
    /// replaces them in place: the box the image takes is re-broken
    /// only if the header now reports a different size, since the
    /// PDF writer reads the asset table fresh on every export and
    /// needs no invalidation to see new pixels at an unchanged size.
    /// An image whose contour was traced is the exception, because
    /// the pixels are what the contour came from.
    /// Only a session that owns its asset table has one to add to;
    /// one that borrowed it says so instead.
    pub fn add_image(&mut self, url: &str, bytes: Vec<u8>) -> Result<Option<u32>, AddImageError> {
        let added = self
            .assets
            .get_mut()
            .ok_or(AddImageError::Borrowed)?
            .add(url, bytes);
        Ok(match added {
            Added::Unchanged(index) => Some(index),
            Added::Replaced {
                index,
                previous,
                current,
            } => {
                if previous != Some(current) {
                    // The table is built with the output and never
                    // patched, so the output goes rather than
                    // outlive the indexes it names. A size that
                    // moved is a box line-breaking reserved
                    // differently, so the section-local cache — keyed
                    // on the url and not on what it resolves to — is
                    // dropped rather than trusted to notice.
                    self.output = None;
                    self.lines.clear();
                    self.settled.clear();
                    self.stale = Stale::Trace;
                } else if self.contours.traces(index) {
                    self.stale = self.stale.max(Stale::Trace);
                }
                Some(index)
            }
            Added::Refused => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Attributes, Block, Book, NodeId, Section};
    use crate::pages::DrawItem;
    use crate::session::Session;
    use crate::session::testing::{
        MAP, book, book_with_image, gif, heading, paragraph, placed, placed_image_size, prose,
        registry, runs, section, sheets, spelled, three_chapters,
    };
    use crate::style::{Source, Stylesheets};

    /// An image the host pushes reaches the display structure: the box is
    /// placed, the asset table names the url, and the pages are
    /// broken again around the room it takes.
    #[test]
    fn an_image_pushed_after_the_book_is_placed_and_indexed() {
        let mut book = Book {
            sections: vec![Section {
                attributes: Default::default(),
                blocks: vec![Block::Image {
                    id: NodeId::UNASSIGNED,
                    url: "plate.jpg".into(),
                    alt: "a map".into(),
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        book.assign_node_ids();
        let mut session =
            Session::owning(crate::fonts::bundled_registry().expect("bundled font parses"));
        session.set_content(book);
        assert!(
            session.preview().assets.is_empty(),
            "a book whose images nobody supplied has no assets",
        );

        assert_eq!(
            session.add_image("plate.jpg", MAP.to_vec()).unwrap(),
            Some(0),
        );
        let output = session.preview();
        assert_eq!(output.assets.len(), 1);
        assert_eq!(output.assets[0].url, "plate.jpg");
        let placed: Vec<&DrawItem> = output
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter(|item| matches!(item, DrawItem::Image { .. }))
            .collect();
        assert_eq!(placed.len(), 1, "the map was not placed");
        assert!(
            output.warnings.is_empty(),
            "a supplied image still complains: {:?}",
            output.warnings,
        );
    }

    /// Registering the same url with the same bytes again costs
    /// nothing: no stage runs again, since neither the asset nor the
    /// box it takes changed.
    #[test]
    fn identical_bytes_at_a_registered_url_cost_nothing() {
        let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
        session.set_content(book_with_image("pic.gif"));
        session.add_image("pic.gif", gif(64, 32, 0)).unwrap();
        session.preview();
        let before = session.stages();

        assert_eq!(
            session.add_image("pic.gif", gif(64, 32, 0)).unwrap(),
            Some(0)
        );
        session.preview();
        assert_eq!(session.stages(), before, "identical bytes cost a stage");
    }

    /// Different bytes at a registered url that probe to the same
    /// size replace the asset without re-breaking anything: the box
    /// an image takes did not move.
    #[test]
    fn a_same_size_replacement_breaks_nothing() {
        let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
        session.set_content(book_with_image("pic.gif"));
        session.add_image("pic.gif", gif(64, 32, 0)).unwrap();
        session.preview();
        let before = session.stages();

        assert_eq!(
            session.add_image("pic.gif", gif(64, 32, 1)).unwrap(),
            Some(0)
        );
        session.preview();
        assert_eq!(
            session.stages().lines,
            before.lines,
            "a same-size replacement broke lines it did not need to"
        );
    }

    /// A different size at a registered url re-breaks the lines
    /// around it, and the new size reaches the page.
    #[test]
    fn a_resized_replacement_re_breaks_and_reaches_the_page() {
        let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
        session.set_content(book_with_image("pic.gif"));
        session.add_image("pic.gif", gif(64, 32, 0)).unwrap();
        let before_size = placed_image_size(session.preview());
        let before_stages = session.stages();

        assert_eq!(
            session.add_image("pic.gif", gif(640, 320, 0)).unwrap(),
            Some(0)
        );
        let after_size = placed_image_size(session.preview());
        assert!(
            session.stages().lines > before_stages.lines,
            "a resize did not re-break the lines around it"
        );
        assert_ne!(
            after_size, before_size,
            "the new size did not reach the page"
        );
    }

    /// A session that borrowed its asset table says so rather than
    /// adding to somebody else's.
    #[test]
    fn a_borrowed_asset_table_refuses_an_image() {
        let mut session = Session::new(registry());
        assert!(matches!(
            session.add_image("plate.jpg", MAP.to_vec()),
            Err(AddImageError::Borrowed),
        ));
    }

    /// The replaceable unit is the file: a host names one, and only
    /// the sections that came from it are broken again.
    #[test]
    fn replacing_a_source_re_breaks_only_its_own_sections() {
        let mut session = three_chapters();
        assert_eq!(
            session.stages().lines,
            3,
            "the first pass broke every section"
        );
        session.replace_source("two.md", vec![section("two.md", prose("delta", 9))]);
        session.preview();
        assert_eq!(
            session.stages().lines,
            4,
            "a section other than two.md was broken again"
        );
    }

    /// One file may split into several sections, and all of them go
    /// when it is replaced.
    #[test]
    fn a_source_that_split_into_several_sections_is_replaced_whole() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![
            section("one.md", vec![heading("One"), paragraph("first")]),
            section("one.md", vec![heading("Two"), paragraph("second")]),
            section("two.md", vec![heading("Three"), paragraph("third")]),
        ]));
        session.preview();
        session.replace_source("one.md", vec![section("one.md", vec![heading("Only")])]);
        session.preview();
        let sources: Vec<Option<&str>> = session
            .book()
            .sections
            .iter()
            .map(|section| section.source.as_deref())
            .collect();
        assert_eq!(sources, vec![Some("one.md"), Some("two.md")]);
        assert_eq!(session.book().sections.len(), 2);
    }

    /// The page name the computed style of one source's section
    /// carries.
    fn page_of(session: &Session, source: &str) -> Option<String> {
        let at = session
            .book()
            .sections
            .iter()
            .position(|section| section.source.as_deref() == Some(source))
            .expect("the book carries the source");
        let tree = session.styles();
        let node = tree
            .nodes()
            .iter()
            .filter(|node| node.element == "section")
            .nth(at)
            .expect("every section is styled");
        tree.styles()[node.style as usize].page.clone()
    }

    /// Acceptance: a host sets a section's class and id without
    /// changing the section's text, and the sheet reaches it by them.
    #[test]
    fn a_host_names_a_section_beside_its_source() {
        let mut session = three_chapters();
        session.set_style(sheets(
            "section.front { page: front } section#closing { page: back }",
        ));
        session.preview();
        let before = session.book().sections.clone();

        session.set_source_attributes(
            "one.md",
            &Attributes {
                id: None,
                classes: vec!["front".into()],
            },
        );
        session.set_source_attributes(
            "three.md",
            &Attributes {
                id: Some("closing".into()),
                classes: Vec::new(),
            },
        );
        session.preview();

        assert_eq!(page_of(&session, "one.md").as_deref(), Some("front"));
        assert_eq!(page_of(&session, "two.md").as_deref(), Some("chapter"));
        assert_eq!(page_of(&session, "three.md").as_deref(), Some("back"));
        for (was, is) in before.iter().zip(&session.book().sections) {
            assert_eq!(was.blocks, is.blocks, "naming a section changed its text");
            assert_eq!(was.span, is.span);
        }

        session.set_source_attributes("one.md", &Attributes::default());
        assert_eq!(page_of(&session, "one.md").as_deref(), Some("chapter"));
    }

    /// A file the book has not seen before arrives at the end.
    #[test]
    fn a_source_the_book_does_not_carry_is_appended() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![section("one.md", vec![paragraph("first")])]));
        session.replace_source("two.md", vec![section("two.md", vec![paragraph("second")])]);
        session.preview();
        assert_eq!(session.book().sections.len(), 2);
    }

    fn sources(session: &Session) -> Vec<String> {
        session
            .book()
            .sections
            .iter()
            .filter_map(|section| section.source.clone())
            .collect()
    }

    /// Acceptance: a source replaced with no sections keeps its place,
    /// and is set there again when it is given some.
    #[test]
    fn a_source_emptied_and_filled_again_keeps_its_place() {
        let mut session = three_chapters();
        session.replace_source("one.md", Vec::new());
        session.preview();
        assert_eq!(sources(&session), ["two.md", "three.md"]);

        session.replace_source("one.md", vec![section("one.md", prose("alpha", 8))]);
        session.preview();
        assert_eq!(sources(&session), ["one.md", "two.md", "three.md"]);

        session.replace_source("two.md", Vec::new());
        session.replace_source("three.md", Vec::new());
        session.replace_source("three.md", vec![section("three.md", prose("gamma", 8))]);
        session.replace_source("two.md", vec![section("two.md", prose("beta", 8))]);
        assert_eq!(sources(&session), ["one.md", "two.md", "three.md"]);
    }

    /// Acceptance: a source with no sections when the book arrives is
    /// set at the place the host named for it.
    #[test]
    fn a_source_empty_when_the_book_arrives_is_set_at_its_place() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![
            section("one.md", prose("alpha", 8)),
            section("three.md", prose("gamma", 8)),
        ]));
        session.set_source_order(vec!["one.md".into(), "two.md".into(), "three.md".into()]);
        session.replace_source("two.md", vec![section("two.md", prose("beta", 8))]);
        session.preview();
        assert_eq!(sources(&session), ["one.md", "two.md", "three.md"]);
    }

    /// A source that was removed has no place to come back to.
    #[test]
    fn a_removed_source_comes_back_at_the_end() {
        let mut session = three_chapters();
        session.remove_source("one.md");
        session.replace_source("one.md", vec![section("one.md", prose("alpha", 8))]);
        assert_eq!(sources(&session), ["two.md", "three.md", "one.md"]);
    }

    /// The cache stores breaks and no positions, so a section the
    /// flow moved paints at new coordinates with the same lines.
    #[test]
    fn a_moved_section_keeps_its_breaks_and_takes_new_coordinates() {
        let mut session = Session::new(registry());
        // Chapters that open where the last one ended, so an edit
        // above moves what follows instead of leaving it on its own
        // opening page.
        session.set_style(sheets("section { break-before: auto }"));
        session.set_content(book(vec![
            section("one.md", prose("alpha", 2)),
            section("two.md", prose("beta", 12)),
        ]));
        let before = runs(session.preview(), "beta");
        let broke = session.stages().lines;

        session.replace_source("one.md", vec![section("one.md", prose("alpha", 24))]);
        let after = runs(session.preview(), "beta");

        assert_eq!(
            session.stages().lines,
            broke + 1,
            "the untouched section was broken again"
        );
        assert!(!before.is_empty(), "the section painted nothing");
        assert_eq!(spelled(&before), spelled(&after), "the breaks moved");
        assert_ne!(placed(&before), placed(&after), "nothing moved");
    }

    /// Setting the same content twice re-keys every section and
    /// breaks none of them: the key is what the section says, not
    /// which node ids it was given this time.
    #[test]
    fn identical_content_set_again_breaks_nothing() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_content(book(vec![
            section("one.md", prose("alpha", 8)),
            section("two.md", prose("beta", 8)),
            section("three.md", prose("gamma", 8)),
        ]));
        session.preview();
        assert_eq!(
            session.stages().lines,
            before.lines,
            "renumbering alone cost a re-break"
        );
    }

    /// A session that owns its registry takes a face after it was
    /// made, and lays out against it: bytes cross once, and the
    /// session they crossed into is the one that keeps them.
    #[test]
    fn an_owning_session_takes_a_face_and_uses_it() {
        let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
        session.set_content(book(vec![section("one.md", prose("alpha", 2))]));
        let faces = session.preview().fonts.len();

        let mut source = crate::fonts::FontSource::from_bytes(crate::fonts::BUNDLED_FONT.to_vec())
            .expect("the bundled face parses");
        source.family = "borrowed garamond".into();
        source.declared = Some(crate::fonts::FaceAttributes::REGULAR);
        let ids = session
            .add_font(source)
            .expect("an owning session registers");
        assert_eq!(ids.len(), 1);

        let css = "book { font-family: 'borrowed garamond' }";
        session.set_style(Stylesheets::parse(&[Source::author("faces.css", css)]));
        let output = session.preview();
        assert_eq!(
            output.fonts.len(),
            faces + 1,
            "the font table did not grow with the registry"
        );
        assert!(
            output
                .pages
                .iter()
                .flat_map(|page| &page.items)
                .any(|item| matches!(
                    item,
                    DrawItem::Text { font_id, .. } if *font_id == ids[0]
                )),
            "the face that was registered set nothing"
        );
    }

    /// A session laying out against someone else's registry has none
    /// of its own to add to, and says so rather than laying out
    /// against a face that is not there.
    #[test]
    fn a_borrowed_registry_refuses_a_face() {
        let mut session = Session::new(registry());
        let source = crate::fonts::FontSource::from_bytes(crate::fonts::BUNDLED_FONT.to_vec())
            .expect("the bundled face parses");
        assert!(matches!(
            session.add_font(source),
            Err(AddFontError::Borrowed)
        ));
    }

    /// A section that moves in the book keeps its lines: the key
    /// travels with the content, and nothing in it is positional.
    #[test]
    fn reordering_the_book_breaks_nothing() {
        let mut session = three_chapters();
        let before = session.stages();
        session.set_content(book(vec![
            section("three.md", prose("gamma", 8)),
            section("one.md", prose("alpha", 8)),
            section("two.md", prose("beta", 8)),
        ]));
        session.preview();
        assert_eq!(
            session.stages().lines,
            before.lines,
            "a section that only moved was broken again"
        );
    }
}
