//! What a host changes: the book, a source, the sheet, a face, an
//! image, the metadata.

use std::borrow::Cow;

use crate::Warning;
use crate::content::{Book, Metadata, Section};
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
        self.book = Cow::Owned(book);
        self.images = has_images(&self.book);
        self.recompile();
        self.stale = Stale::Trace;
    }

    /// Replaces every section that came from one source file.
    ///
    /// The source is the replaceable unit because a host names files
    /// and one file may split into several sections. A name the book
    /// does not already have appends instead, which is how a file it
    /// has not seen before arrives.
    pub fn replace_source(&mut self, name: &str, sections: Vec<Section>) {
        let mut sections = sections;
        let book = self.book.to_mut();
        let mut rebuilt = Vec::with_capacity(book.sections.len() + sections.len());
        let mut placed = false;
        for section in std::mem::take(&mut book.sections) {
            if section.source.as_deref() == Some(name) {
                if !placed {
                    rebuilt.append(&mut sections);
                    placed = true;
                }
            } else {
                rebuilt.push(section);
            }
        }
        if !placed {
            rebuilt.append(&mut sections);
        }
        book.sections = rebuilt;
        book.assign_node_ids();
        self.images = has_images(&self.book);
        self.recompile();
        self.stale = Stale::Trace;
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
        let ids = self
            .registry
            .get_mut()
            .ok_or(AddFontError::Borrowed)?
            .add(source)?;
        // The table is built with the output and never patched, so
        // the output goes rather than outlive the ids it indexes.
        self.output = None;
        self.stale = Stale::Break;
        self.recompile();
        Ok(ids)
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

    /// A file the book has not seen before arrives at the end.
    #[test]
    fn a_source_the_book_does_not_carry_is_appended() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![section("one.md", vec![paragraph("first")])]));
        session.replace_source("two.md", vec![section("two.md", vec![paragraph("second")])]);
        session.preview();
        assert_eq!(session.book().sections.len(), 2);
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
