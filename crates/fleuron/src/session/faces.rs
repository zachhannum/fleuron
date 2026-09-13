//! The files a sheet's `@font-face` rules name, and the faces a
//! session registers from them.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::Warning;
use crate::style::FontLoader;

use super::{Session, Stale};

/// Font files, by the url a `@font-face` names them by.
///
/// The faces registered from them come after every face the host
/// registered directly, so dropping them leaves the host's ids where
/// they were.
pub(super) struct FontFiles {
    files: HashMap<String, (u64, Vec<u8>)>,
    /// How many faces the host registered directly.
    base: usize,
    /// What the sheets and the files hashed to when their faces were
    /// last registered.
    print: Option<u64>,
    /// What registering them warned about.
    warnings: Vec<Warning>,
}

impl FontFiles {
    pub(super) fn over(base: usize) -> FontFiles {
        FontFiles {
            files: HashMap::new(),
            base,
            print: None,
            warnings: Vec::new(),
        }
    }

    /// Keeps one file, and says whether its bytes are new.
    fn insert(&mut self, url: &str, bytes: Vec<u8>) -> bool {
        let mut h = DefaultHasher::new();
        bytes.hash(&mut h);
        let hash = h.finish();
        if self.files.get(url).is_some_and(|(known, _)| *known == hash) {
            return false;
        }
        self.files.insert(url.to_string(), (hash, bytes));
        true
    }

    fn hash(&self, url: &str) -> Option<u64> {
        self.files.get(url).map(|(hash, _)| *hash)
    }
}

impl FontLoader for FontFiles {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        self.files.get(url).map(|(_, bytes)| bytes.clone())
    }
}

impl Session<'_> {
    /// Keeps a font file under the url a `@font-face` names it by,
    /// and registers whatever the sheet declares from it.
    ///
    /// A url no rule names is kept until one does, so the file and
    /// the sheet can arrive in either order. The same bytes at the
    /// same url again cost nothing.
    pub fn add_font_file(&mut self, url: &str, bytes: Vec<u8>) -> Result<(), super::AddFontError> {
        if self.registry.get_mut().is_none() {
            return Err(super::AddFontError::Borrowed);
        }
        if self.fonts.insert(url, bytes) && self.load_faces() {
            self.recompile();
        }
        Ok(())
    }

    /// Registers a face the host names by its file alone, under the
    /// faces the sheet declares.
    pub(super) fn register_font(
        &mut self,
        source: crate::fonts::FontSource,
    ) -> Result<Vec<u16>, super::AddFontError> {
        let registry = self
            .registry
            .get_mut()
            .ok_or(super::AddFontError::Borrowed)?;
        registry.truncate(self.fonts.base);
        let added = registry.add(source);
        self.fonts.base = registry.len();
        self.fonts.print = None;
        self.load_faces();
        Ok(added?)
    }

    /// Registers the faces the sheets declare, from the files the
    /// host handed over, and says whether they changed. Sheets that
    /// declare the faces already registered register nothing.
    pub(super) fn load_faces(&mut self) -> bool {
        let (Some(sheets), Some(registry)) = (&mut self.sheets, self.registry.get_mut()) else {
            return false;
        };
        let mut h = DefaultHasher::new();
        sheets.hash_faces(&mut h, |url| self.fonts.hash(url));
        let print = h.finish();
        if self.fonts.print == Some(print) {
            sheets.set_font_warnings(self.fonts.warnings.clone());
            return false;
        }
        registry.truncate(self.fonts.base);
        sheets.load_fonts(registry, &self.fonts);
        self.fonts.warnings = sheets.font_warnings().to_vec();
        self.fonts.print = Some(print);
        // An id that stood before can name another file now, and a
        // section's key names the id, so no cached line survives.
        self.output = None;
        self.lines.clear();
        self.settled.clear();
        self.stale = self.stale.max(Stale::Break);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::LayoutOutput;
    use crate::fonts::{FaceAttributes, FontSource, bundled_registry};
    use crate::pages::DrawItem;
    use crate::session::testing::{FELL, book, prose, registry, section, sheets};
    use crate::session::{AddFontError, Session};
    use crate::style::{NoFonts, Source, Stylesheets};

    const FELL_CAPS: &str = "@font-face { font-family: 'Fell Caps'; src: url(fell.ttf); \
        font-weight: 700; font-style: italic }\n\
        p { font-family: 'Fell Caps', serif; font-weight: 700; font-style: italic }";

    fn session() -> Session<'static> {
        let mut session = Session::owning(bundled_registry().unwrap());
        session.set_content(book(vec![section("one.md", prose("alpha", 4))]));
        session
    }

    /// Every face a text run on any page names.
    fn used(output: &LayoutOutput) -> BTreeSet<u16> {
        output
            .pages
            .iter()
            .flat_map(|page| &page.items)
            .filter_map(|item| match item {
                DrawItem::Text { font_id, .. } => Some(*font_id),
                _ => None,
            })
            .collect()
    }

    fn family(output: &LayoutOutput, family: &str) -> Vec<u16> {
        (0..output.fonts.len() as u16)
            .filter(|id| output.fonts[usize::from(*id)].family == family)
            .collect()
    }

    /// A rule whose url has bytes registers one face, under the
    /// family, the weight and the slope the rule declares.
    #[test]
    fn a_font_face_with_bytes_registers_what_the_rule_declares() {
        let mut session = session();
        session.add_font_file("fell.ttf", FELL.to_vec()).unwrap();
        session.set_style(sheets(FELL_CAPS));
        let output = session.preview();
        let fell = family(output, "fell caps");
        assert_eq!(fell.len(), 1, "{:?}", output.fonts);
        assert_eq!(
            output.fonts[usize::from(fell[0])].attributes,
            FaceAttributes {
                italic: true,
                weight: 700
            },
        );
        assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    }

    /// Text that asks for the family is set in that face, and the
    /// export writes from the same pages.
    #[test]
    fn text_that_asks_for_the_family_sets_in_it() {
        let mut session = session();
        session.add_font_file("fell.ttf", FELL.to_vec()).unwrap();
        session.set_style(sheets(FELL_CAPS));
        let output = session.preview();
        let fell = family(output, "fell caps");
        assert_eq!(used(output), fell.into_iter().collect());
        assert!(session.export().is_ok());
    }

    /// A rule whose url has no bytes warns the way loading the same
    /// sheet through a loader that resolves nothing warns.
    #[test]
    fn a_font_face_without_bytes_warns_as_a_loader_does() {
        let mut session = session();
        session.set_style(sheets(FELL_CAPS));
        let output = session.preview();

        let mut loaded = sheets(FELL_CAPS);
        loaded.load_fonts(&mut bundled_registry().unwrap(), &NoFonts);
        assert!(!loaded.warnings().is_empty());
        for warning in loaded.warnings() {
            assert!(output.warnings.contains(warning), "{warning:?} is missing");
        }
    }

    /// Bytes that arrive after the sheet register the face, and the
    /// text that fell back sets in it.
    #[test]
    fn a_face_whose_bytes_arrive_after_the_sheet_sets_in_it() {
        let mut session = session();
        session.set_style(sheets(FELL_CAPS));
        let before = session.preview();
        assert!(family(before, "fell caps").is_empty());
        assert!(!before.warnings.is_empty());

        session.add_font_file("fell.ttf", FELL.to_vec()).unwrap();
        let after = session.preview();
        let fell = family(after, "fell caps");
        assert_eq!(used(after), fell.into_iter().collect());
        assert!(after.warnings.is_empty(), "{:?}", after.warnings);
    }

    /// The same sheet set again registers no face again and breaks
    /// no line again, and still warns about a rule with no bytes.
    #[test]
    fn the_same_sheet_again_registers_nothing_again() {
        let css =
            format!("{FELL_CAPS}\n@font-face {{ font-family: Absent; src: url(absent.ttf) }}");
        let mut session = session();
        session.add_font_file("fell.ttf", FELL.to_vec()).unwrap();
        session.set_style(sheets(&css));
        let warnings = session.preview().warnings.clone();
        let faces = session.fonts().len();
        let before = session.stages();

        session.set_style(sheets(&css));
        session.add_font_file("fell.ttf", FELL.to_vec()).unwrap();
        let output = session.preview();
        assert_eq!(output.warnings, warnings);
        assert_eq!(session.fonts().len(), faces);
        assert_eq!(session.stages().lines, before.lines);
    }

    /// A rule renamed replaces the face it registered, and a rule
    /// removed takes its face with it.
    #[test]
    fn changing_or_removing_a_rule_changes_the_faces_used() {
        let mut session = session();
        session.add_font_file("fell.ttf", FELL.to_vec()).unwrap();
        session.set_style(sheets(FELL_CAPS));
        let bundled = session.preview().fonts.len() - 1;

        session.set_style(sheets(&FELL_CAPS.replace("Fell Caps", "Oxford")));
        let renamed = session.preview();
        assert!(family(renamed, "fell caps").is_empty());
        let oxford = family(renamed, "oxford");
        assert_eq!(used(renamed), oxford.into_iter().collect());

        session.set_style(sheets("p { font-family: 'Fell Caps', serif }"));
        let removed = session.preview();
        assert_eq!(removed.fonts.len(), bundled);
        assert!(used(removed).iter().all(|id| usize::from(*id) < bundled));
    }

    /// A face the host registers by its file after the sheet's faces
    /// takes the next id after the host's own, and the sheet's face
    /// still sets.
    #[test]
    fn a_face_the_host_adds_goes_under_the_sheets_faces() {
        let mut session = session();
        let bundled = session.fonts().len() as u16;
        session.add_font_file("fell.ttf", FELL.to_vec()).unwrap();
        session.set_style(sheets(FELL_CAPS));
        session.preview();

        let ids = session
            .add_font(FontSource::from_bytes(FELL.to_vec()).unwrap())
            .unwrap();
        assert_eq!(ids, vec![bundled]);
        let output = session.preview();
        let fell = family(output, "fell caps");
        assert_eq!(fell, vec![bundled + 1]);
        assert_eq!(used(output), fell.into_iter().collect());
    }

    /// A session that borrowed its registry has no faces of its own to
    /// register from a file.
    #[test]
    fn a_borrowed_registry_refuses_a_font_file() {
        let mut session = Session::new(registry());
        session.set_style(Stylesheets::parse(&[Source::author("a.css", FELL_CAPS)]));
        assert!(matches!(
            session.add_font_file("fell.ttf", FELL.to_vec()),
            Err(AddFontError::Borrowed)
        ));
    }
}
