//! fleuron-wasm: layout in a worker, bytes out.
//!
//! The door between the engine and a host: markdown or a content
//! tree and CSS text in, one buffer out: the postcard display structure,
//! or PDF bytes on the export path. The engine never touches the
//! DOM, opens nothing, and reads no clock.
//!
//! The surface is a [`Session`] rather than a function, because the
//! event a preview is built around is a small change to one input
//! while the others stand. Inputs cross when they change: a face is
//! registered once and stays registered, a manuscript crosses once
//! and a keystroke replaces the one file it touched, and a
//! stylesheet crosses as CSS text on its own. [`render`] is the
//! batch case, and it is the same session used once.
//!
//! Nothing here deals with generations or workers. A render runs to
//! completion, so a superseded one is dropped by the host that asked
//! for it rather than interrupted half-way through a stage; the
//! protocol that does the dropping is in the TypeScript beside this
//! crate.

#![deny(missing_docs)]

use std::collections::BTreeMap;

use fleuron::Warning;
use fleuron::content::{Book, HeadingLevel, Metadata};
use fleuron::fonts::{FontSource, bundled_registry};
use fleuron::session::Session as Engine;
use fleuron::style::{Source, Stylesheets};
use fleuron::wire;
use fleuron_markdown::{Dialect, Options, Sections};
use wasm_bindgen::prelude::*;

/// The version the display structure is encoded at. A host reads the same
/// number off the front of every buffer and refuses one it does not
/// know.
#[wasm_bindgen(js_name = wireVersion)]
pub fn wire_version() -> u16 {
    wire::VERSION
}

/// A retained pipeline, kept in the module between calls.
///
/// The session keeps the content tree, the styling and every stage
/// between them and the page, so a second render pays for what
/// changed and not for the book. What the stages cost, and what
/// survives which edit, is the engine's own contract.
#[wasm_bindgen]
pub struct Session {
    engine: Engine<'static>,
    reading: Options,
    /// What reading each source complained about, kept per source so
    /// that replacing one file replaces its complaints and no
    /// others.
    complaints: BTreeMap<String, Vec<Warning>>,
}

#[wasm_bindgen]
impl Session {
    /// A session over the bundled face, with no content and the
    /// built-in sheet alone.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Session, JsError> {
        let registry = bundled_registry().map_err(js_error)?;
        Ok(Session {
            engine: Engine::owning(registry),
            reading: Options::default(),
            complaints: BTreeMap::new(),
        })
    }

    /// Registers a face from font bytes, and returns the ids it was
    /// registered under. A variable file names several cuts and
    /// yields one id each.
    ///
    /// The bytes stay in the module. A host sends a face once, not
    /// once per render.
    #[wasm_bindgen(js_name = addFont)]
    pub fn add_font(&mut self, bytes: &[u8]) -> Result<Vec<u16>, JsError> {
        let source = FontSource::from_bytes(bytes.to_vec()).map_err(js_error)?;
        self.engine.add_font(source).map_err(js_error)
    }

    /// Which markdown the sources are written in: `commonmark`,
    /// `gfm` or `obsidian`.
    #[wasm_bindgen(js_name = setDialect)]
    pub fn set_dialect(&mut self, dialect: &str) -> Result<(), JsError> {
        self.reading.dialect = match dialect {
            "commonmark" => Dialect::common_mark(),
            "gfm" => Dialect::gfm(),
            "obsidian" => Dialect::obsidian(),
            other => return Err(JsError::new(&format!("unknown dialect {other}"))),
        };
        Ok(())
    }

    /// Where a source's sections begin: at a heading of this level
    /// or shallower, or nowhere at all when the level is zero, which
    /// makes each file one section.
    ///
    /// Sections are what a page can start on, so this sets a book's
    /// page count before any styling does.
    #[wasm_bindgen(js_name = setSplit)]
    pub fn set_split(&mut self, level: u8) -> Result<(), JsError> {
        self.reading.sections = match level {
            0 => Sections::Whole,
            level => Sections::AtHeading(HeadingLevel::try_from(level).map_err(js_error)?),
        };
        Ok(())
    }

    /// Reads one markdown source as the whole book, its frontmatter
    /// the book's metadata.
    ///
    /// Everything below box construction is invalidated: this is the
    /// manuscript arriving, not an edit to it.
    #[wasm_bindgen(js_name = setMarkdown)]
    pub fn set_markdown(&mut self, name: &str, text: &str) {
        let metadata = fleuron_markdown::frontmatter(text);
        let (sections, complaints) = fleuron_markdown::to_sections(text, name, &self.reading);
        self.engine
            .set_content(fleuron_markdown::assemble(metadata, sections));
        self.complaints.clear();
        self.complain(name, complaints);
    }

    /// Reads several markdown sources as one book, in the order
    /// given.
    ///
    /// A lone source is the whole book, so its frontmatter is the
    /// book's. Several are chapters: each file's frontmatter belongs
    /// to the section it became, and the book is left unnamed rather
    /// than named after whichever chapter came first, which is what
    /// [`Session::set_metadata`] is for.
    #[wasm_bindgen(js_name = setSources)]
    pub fn set_sources(&mut self, names: Vec<String>, texts: Vec<String>) -> Result<(), JsError> {
        if names.len() != texts.len() {
            return Err(JsError::new(&format!(
                "{} sources named and {} handed over",
                names.len(),
                texts.len()
            )));
        }
        let metadata = match texts.as_slice() {
            [whole] => fleuron_markdown::frontmatter(whole),
            _ => Metadata::default(),
        };
        self.complaints.clear();
        let mut sections = Vec::new();
        for (name, text) in names.iter().zip(&texts) {
            let (read, complaints) = fleuron_markdown::to_sections(text, name, &self.reading);
            sections.extend(read);
            self.complaints.insert(name.clone(), complaints);
        }
        self.engine
            .set_content(fleuron_markdown::assemble(metadata, sections));
        self.reflect();
        Ok(())
    }

    /// Drops every section that came from one source, and the
    /// complaints reading it raised.
    #[wasm_bindgen(js_name = removeMarkdown)]
    pub fn remove_markdown(&mut self, name: &str) {
        self.engine.replace_source(name, Vec::new());
        self.complaints.remove(name);
        self.reflect();
    }

    /// Names the book, from JSON: `title`, `author`, and an `extra`
    /// object for whatever else a frontend read.
    ///
    /// A book read from several sources has no frontmatter of its
    /// own, so this is how it gets a title. Nothing between the
    /// content tree and the page reads metadata, so a book renamed
    /// between renders re-runs no stage; the PDF writer is the one
    /// thing that reads it.
    #[wasm_bindgen(js_name = setMetadata)]
    pub fn set_metadata(&mut self, json: &str) -> Result<(), JsError> {
        let metadata: Metadata = serde_json::from_str(json).map_err(js_error)?;
        self.engine.set_metadata(metadata);
        Ok(())
    }

    /// Replaces every section that came from one source, reparsing
    /// that source alone. A name the book does not already have
    /// appends instead, which is how a file it has not seen before
    /// arrives.
    ///
    /// This is the keystroke path: one file crosses, one file is
    /// read, and every other section keeps the lines it already has.
    #[wasm_bindgen(js_name = updateMarkdown)]
    pub fn update_markdown(&mut self, name: &str, text: &str) {
        let (sections, complaints) = fleuron_markdown::to_sections(text, name, &self.reading);
        self.engine.replace_source(name, sections);
        self.complain(name, complaints);
    }

    /// Sets the book from a content tree, as JSON.
    ///
    /// Markdown is the way in; this is the door for a host with a
    /// structured source of its own. Node ids are the engine's and
    /// are assigned on the way in, so a tree built by hand needs
    /// none.
    #[wasm_bindgen(js_name = setContent)]
    pub fn set_content(&mut self, json: &str) -> Result<(), JsError> {
        let book: Book = serde_json::from_str(json).map_err(js_error)?;
        self.engine.set_content(book);
        self.complaints.clear();
        self.engine.set_source_warnings(Vec::new());
        Ok(())
    }

    /// Sets the author styling from CSS text, cascading over the
    /// built-in sheet.
    ///
    /// Which stages this costs is the change's own business: a
    /// colour repaints nothing, page geometry re-fragments over the
    /// lines already broken, and only the measure or the face breaks
    /// them again.
    #[wasm_bindgen(js_name = setStyle)]
    pub fn set_style(&mut self, css: &str) {
        self.engine
            .set_style(Stylesheets::parse(&[Source::author("author.css", css)]));
    }

    /// Registers one image by the url the content tree names it by,
    /// and returns the index `DrawItem::Image.asset` gets for it.
    /// `undefined` for bytes no probe recognises, which is a
    /// diagnostic on the next display structure and no image.
    ///
    /// The engine opens nothing, and a worker has nothing to open:
    /// the host fetches the file and hands the bytes over, the same as
    /// with a face. The bytes stay in the module, so an image crosses
    /// once rather than once per render.
    #[wasm_bindgen(js_name = addImage)]
    pub fn add_image(&mut self, url: &str, bytes: &[u8]) -> Result<Option<u32>, JsError> {
        self.engine.add_image(url, bytes.to_vec()).map_err(js_error)
    }

    /// The file a face was registered from, for a painter that has
    /// to draw with the bytes the engine shaped with.
    ///
    /// The bundled face is the case that needs this: it is inside
    /// the module and there is no URL a host could fetch it from.
    /// A variable file answers for every cut it named, so the same
    /// bytes come back for each of them, and the face's variations
    /// on the display structure say which instance to draw.
    #[wasm_bindgen(js_name = fontBytes)]
    pub fn font_bytes(&self, font_id: u16) -> Result<Vec<u8>, JsError> {
        self.engine
            .fonts()
            .bytes(font_id)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| JsError::new(&format!("no face is registered as font {font_id}")))
    }

    /// The display structure, postcard-encoded, version first.
    pub fn preview(&mut self) -> Result<Vec<u8>, JsError> {
        wire::encode(self.engine.preview()).map_err(js_error)
    }

    /// The same run as PDF bytes. Both painters read the stages the
    /// preview settled, so an export cannot contradict what is on
    /// screen.
    #[wasm_bindgen(js_name = exportPdf)]
    pub fn export_pdf(&mut self) -> Result<Vec<u8>, JsError> {
        self.engine.export().map_err(js_error)
    }

    /// How many times each stage has run since the session was made,
    /// as `[style, lines, flow, paint]`.
    ///
    /// What an edit cost, said in stage runs rather than
    /// milliseconds: a host watching this can see a cache serve
    /// where a clock would only see a fast machine.
    pub fn stages(&self) -> Vec<u32> {
        let stages = self.engine.stages();
        vec![stages.style, stages.lines, stages.flow, stages.paint]
    }
}

impl Session {
    /// Files one source's complaints and hands the engine the lot,
    /// so that what the frontend said comes back on the display structure
    /// beside what the styling and the layout said.
    fn complain(&mut self, name: &str, complaints: Vec<Warning>) {
        self.complaints.insert(name.to_string(), complaints);
        self.reflect();
    }

    /// Hands the engine every source's complaints as they stand.
    fn reflect(&mut self) {
        self.engine
            .set_source_warnings(self.complaints.values().flatten().cloned().collect());
    }
}

/// One markdown source and one stylesheet, laid out once: the batch
/// case, over the same session a live preview keeps.
#[wasm_bindgen]
pub fn render(markdown: &str, css: &str) -> Result<Vec<u8>, JsError> {
    once(markdown, css)?.preview()
}

/// The same, as PDF bytes.
#[wasm_bindgen(js_name = renderPdf)]
pub fn render_pdf(markdown: &str, css: &str) -> Result<Vec<u8>, JsError> {
    once(markdown, css)?.export_pdf()
}

fn once(markdown: &str, css: &str) -> Result<Session, JsError> {
    let mut session = Session::new()?;
    session.set_markdown("book.md", markdown);
    if !css.is_empty() {
        session.set_style(css);
    }
    Ok(session)
}

/// Anything that went wrong, in the host's own error type. The
/// engine's errors all say one line about themselves.
fn js_error(error: impl std::fmt::Display) -> JsError {
    JsError::new(&error.to_string())
}
