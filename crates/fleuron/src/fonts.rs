//! The font registry: font bytes in, `font_id`s out.
//!
//! Both the shaper and every painter resolve glyphs through this one
//! table, which is what makes preview equal export. The registry is
//! the sole owner of font bytes; nothing else in the engine holds a
//! `FontRef` lifetime.

use std::collections::HashMap;
use std::sync::Arc;

use harfrust::{
    BufferClusterLevel, FontRef as HarfFontRef, Language, ShaperData, ShaperInstance,
    UnicodeBuffer, script,
};
use serde::Serialize;
use skrifa::MetadataProvider;
use skrifa::attribute::Style as SlopeStyle;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::metrics::{GlyphMetrics, Metrics};
use skrifa::prelude::GlyphId;
use skrifa::string::StringId;

/// A font entering the engine: raw bytes and the identity the style
/// tree matched against.
///
/// Bytes are opaque here — layout never decodes images or fonts
/// twice; decoding happens once, at registration.
#[derive(Debug, Clone)]
pub struct FontSource {
    /// The full font file (TTF/OTF). Registry-owned; callers hand
    /// over a copy.
    pub bytes: Vec<u8>,
    /// Family name for matching (lowercase, e.g. "eb garamond").
    pub family: String,
    /// Face name (name id 4), e.g. "EB Garamond Regular".
    pub name: String,
    /// Style name (name id 2), e.g. "Regular".
    pub style: String,
    /// What a stylesheet declared this face to be, overriding what
    /// the file says about itself. A declared identity also pins
    /// registration to the file's default instance: a sheet naming
    /// one slope and one weight is not describing a variable
    /// family's five.
    pub declared: Option<FaceAttributes>,
}

impl FontSource {
    /// Builds a source from a font file, reading identity from its
    /// name table.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, FontError> {
        let font = skrifa::FontRef::new(&bytes).map_err(|_| FontError::Parse)?;
        let family = name_string(&font, StringId::TYPOGRAPHIC_FAMILY_NAME)
            .or_else(|| name_string(&font, StringId::FAMILY_NAME))
            .ok_or(FontError::MissingName)?;
        let name = name_string(&font, StringId::FULL_NAME).unwrap_or_else(|| family.clone());
        let style =
            name_string(&font, StringId::SUBFAMILY_NAME).unwrap_or_else(|| "Regular".into());
        Ok(FontSource {
            bytes,
            family: family.to_lowercase(),
            name,
            style,
            declared: None,
        })
    }
}

/// The slope and weight a face is matched at: the two axes of CSS
/// font matching the engine supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FaceAttributes {
    /// True for both italic and oblique cuts; the engine draws no
    /// distinction a book needs.
    pub italic: bool,
    /// Weight on the CSS 1–1000 scale.
    pub weight: u16,
}

impl FaceAttributes {
    /// Upright, regular: what a face is when nothing says otherwise.
    pub const REGULAR: FaceAttributes = FaceAttributes {
        italic: false,
        weight: 400,
    };
}

/// One axis of a variable face, pinned: the tag and the user-space
/// coordinate this face sits at.
///
/// A face at its family's default location carries none — there is
/// nothing to pin, and an instanced subset of the default is just
/// the default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AxisSetting {
    /// The four-byte OpenType axis tag, e.g. `wght`.
    pub tag: [u8; 4],
    /// The coordinate in the axis's own units.
    pub value: f32,
}

/// The face a request for a family, slope and weight resolved to,
/// and what that face actually is.
///
/// The two differ when the family has no cut at the slope or weight
/// asked for; the caller decides whether that is worth a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaceMatch {
    /// The registry id to shape and paint with.
    pub id: u16,
    /// What that face is, which need not be what was asked for.
    pub attributes: FaceAttributes,
}

/// A generic family keyword, as in CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericFamily {
    /// `serif`
    Serif,
    /// `sans-serif`
    SansSerif,
    /// `monospace`
    Monospace,
}

impl GenericFamily {
    /// The keyword as it appears in stylesheets.
    pub fn keyword(self) -> &'static str {
        match self {
            GenericFamily::Serif => "serif",
            GenericFamily::SansSerif => "sans-serif",
            GenericFamily::Monospace => "monospace",
        }
    }

    /// Parses a stylesheet keyword, case-insensitively; `None` means
    /// the value isn't a generic.
    pub fn parse(keyword: &str) -> Option<Self> {
        match keyword.to_ascii_lowercase().as_str() {
            "serif" => Some(GenericFamily::Serif),
            "sans-serif" | "sans" => Some(GenericFamily::SansSerif),
            "monospace" | "mono" => Some(GenericFamily::Monospace),
            _ => None,
        }
    }
}

/// One shaped glyph: its id, its advance, and the byte offset of its
/// cluster in the shaped string — the bridge between shaping output
/// and text-anchored break opportunities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// Glyph id in the face that shaped it.
    pub id: u32,
    /// Horizontal advance in font units.
    pub x_advance: u32,
    /// Byte index into the shaped string where this glyph's cluster
    /// begins.
    pub cluster: u32,
}

/// A font's identity, as carried in the engine's output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FontRefEntry {
    /// Family for matching (lowercase).
    pub family: String,
    /// Face name (name id 4).
    pub name: String,
    /// Style name (name id 2).
    pub style: String,
    /// The slope and weight this face answers for.
    pub attributes: FaceAttributes,
}

/// Font metrics in font units.
///
/// Ascender/descender/line gap follow the OS/2 typographic values
/// when present, hhea otherwise. Descender is negative, per the
/// tables.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FontMetricsTable {
    /// Font design units per em; everything else here is in them.
    pub units_per_em: u16,
    /// Height above the baseline.
    pub ascender: i16,
    /// Depth below the baseline, negative.
    pub descender: i16,
    /// Leading the face asks for between lines.
    pub line_gap: i16,
    /// Height of a capital above the baseline. Zero when the face
    /// declares none, which is what a drop cap has to fall back from.
    pub cap_height: i16,
}

/// One registered face: bytes plus everything decoded from them.
///
/// A variable file registers one face per named instance, so bytes
/// and the shaper's per-file tables are shared and only the location
/// differs.
struct Face {
    bytes: Arc<Vec<u8>>,
    identity: FontRefEntry,
    metrics: FontMetricsTable,
    /// Decoded once, at registration; the shaper reads through this.
    shaper_data: Arc<ShaperData>,
    /// Where on the file's axes this face sits, normalized. Default
    /// for a static file.
    location: Location,
    /// The same location in user space, for painters that instance
    /// the file themselves. Empty at the default location.
    variations: Vec<AxisSetting>,
    /// The shaper's view of `location`; `None` at the default, where
    /// there is nothing to vary.
    instance: Option<ShaperInstance>,
}

/// The registry: assigns `font_id`s, hands shaper access and metrics
/// to the pipeline, and maps generic families to bundled faces.
///
/// Ids are dense and sequential from 0 — an id is an index.
#[derive(Default)]
pub struct FontRegistry {
    faces: Vec<Face>,
    by_family: HashMap<String, Vec<u16>>,
    generics: HashMap<GenericFamily, u16>,
}

impl FontRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a font file and returns the ids of the faces it
    /// yielded, in the order the file names them.
    ///
    /// A variable file yields one face per named instance: the cuts
    /// the family says it has. Matching is then a lookup over faces
    /// with fixed slopes and weights, which is what lets the cascade
    /// resolve a face without a registry it may write to.
    ///
    /// Decodes metrics eagerly: a font that can't be parsed fails
    /// here, once, rather than at first shape.
    pub fn add(&mut self, mut source: FontSource) -> Result<Vec<u16>, FontError> {
        let bytes = Arc::new(std::mem::take(&mut source.bytes));
        let font = skrifa::FontRef::new(&bytes).map_err(|_| FontError::Parse)?;
        let shaper_data = Arc::new(ShaperData::new(&font));
        let harf = HarfFontRef::new(&bytes).map_err(|_| FontError::Parse)?;

        let mut ids = Vec::new();
        for cut in cuts(&font, &source) {
            let id = self.faces.len() as u16;
            let instance = (!cut.variations.is_empty())
                .then(|| ShaperInstance::from_coords(&harf, cut.location.coords().iter().copied()));
            self.by_family
                .entry(source.family.clone())
                .or_default()
                .push(id);
            self.faces.push(Face {
                bytes: bytes.clone(),
                identity: FontRefEntry {
                    family: source.family.clone(),
                    name: cut.name,
                    style: cut.style,
                    attributes: cut.attributes,
                },
                metrics: read_metrics(&font, &cut.location),
                shaper_data: shaper_data.clone(),
                location: cut.location,
                variations: cut.variations,
                instance,
            });
            ids.push(id);
        }
        Ok(ids)
    }

    /// Number of registered faces. Ids are `0..len`.
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    /// True when no face is registered.
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// Binds a generic keyword to a registered family's first face.
    /// The keyword must not already be bound.
    pub fn map_generic(&mut self, generic: GenericFamily, family: &str) -> Option<u16> {
        let id = self.by_family.get(family)?.first().copied()?;
        if self.generics.insert(generic, id).is_some() {
            panic!("generic family {generic:?} already mapped");
        }
        Some(id)
    }

    /// The id a generic keyword resolves to.
    pub fn generic(&self, generic: GenericFamily) -> Option<u16> {
        self.generics.get(&generic).copied()
    }

    /// The id of the first face whose family matches (case-insensitive
    /// compare against the lowercased registry key).
    pub fn by_family(&self, family: &str) -> Option<u16> {
        self.by_family
            .get(&family.to_lowercase())
            .and_then(|ids| ids.first().copied())
    }

    /// The face of `family` that best answers a slope and weight.
    ///
    /// Matching follows CSS: slope decides first — a family with an
    /// italic cut never answers an italic request with the upright —
    /// and weight is then chosen by the desired-weight rules, which
    /// look up before they look down in the text range and away from
    /// it elsewhere. The match reports what it actually found, so a
    /// caller can say when the family had nothing at the slope asked
    /// for.
    pub fn select(&self, family: &str, want: FaceAttributes) -> Option<FaceMatch> {
        let ids = self.by_family.get(&family.to_lowercase())?;
        let attributes = |id: &u16| self.faces[*id as usize].identity.attributes;
        // A family with nothing at the slope asked for answers with
        // everything it has; one that has it answers only with that.
        let has_slope = ids.iter().any(|id| attributes(id).italic == want.italic);
        let id = ids
            .iter()
            .filter(|id| !has_slope || attributes(id).italic == want.italic)
            .min_by_key(|id| (weight_rank(attributes(id).weight, want.weight), **id))
            .copied()?;
        Some(FaceMatch {
            id,
            attributes: attributes(&id),
        })
    }

    /// Identity of a face, for the output font table.
    pub fn font_ref(&self, id: u16) -> Option<&FontRefEntry> {
        self.faces.get(id as usize).map(|face| &face.identity)
    }

    /// Metrics of a face, in font units.
    pub fn metrics(&self, id: u16) -> Option<FontMetricsTable> {
        self.faces.get(id as usize).map(|face| face.metrics)
    }

    /// The raw bytes of a face, for embedding at write time.
    ///
    /// Shared: the faces a variable file yielded are one file, and a
    /// painter embeds one copy of it however many cuts it draws.
    pub fn bytes(&self, id: u16) -> Option<Arc<Vec<u8>>> {
        self.faces.get(id as usize).map(|face| face.bytes.clone())
    }

    /// Where on its file's axes a face sits, in user space. Empty
    /// for a static face and for a variable one at its default
    /// location.
    pub fn variations(&self, id: u16) -> Option<&[AxisSetting]> {
        self.faces
            .get(id as usize)
            .map(|face| face.variations.as_slice())
    }

    /// Advance width of one glyph, in font units.
    pub fn advance_width(&self, id: u16, glyph: u32) -> Option<u16> {
        let face = self.faces.get(id as usize)?;
        let font = HarfFontRef::new(&face.bytes).ok()?;
        let glyph_metrics =
            GlyphMetrics::new(&font, Size::unscaled(), LocationRef::from(&face.location));
        glyph_metrics
            .advance_width(GlyphId::new(glyph))
            .map(|w| w.round() as u16)
    }

    /// The advance widths of a run of glyphs, in font units.
    ///
    /// Shaped output carries glyph ids; measuring them must not
    /// re-decode the font per glyph, so this batches.
    pub fn advance_widths(&self, id: u16, glyphs: &[u32]) -> Option<Vec<u16>> {
        let face = self.faces.get(id as usize)?;
        let font = HarfFontRef::new(&face.bytes).ok()?;
        let glyph_metrics =
            GlyphMetrics::new(&font, Size::unscaled(), LocationRef::from(&face.location));
        Some(
            glyphs
                .iter()
                .map(|g| {
                    glyph_metrics
                        .advance_width(GlyphId::new(*g))
                        .map(|w| w.round() as u16)
                        .unwrap_or(0)
                })
                .collect(),
        )
    }

    /// Maps one character to its nominal glyph, in font units.
    pub fn char_glyph(&self, id: u16, ch: char) -> Option<u32> {
        let face = self.faces.get(id as usize)?;
        let font = HarfFontRef::new(&face.bytes).ok()?;
        let charmap = font.charmap();
        charmap.map(ch).map(|g| g.to_u32())
    }

    /// Shapes a string with a registered face, in font units.
    ///
    /// Returns per-glyph `ShapedGlyph` — the raw material of line
    /// layout. Clusters index the shaped string, so callers can map
    /// text offsets (break opportunities) back to glyphs; offsets data
    /// stays in the shaper's buffer.
    pub fn shape(&self, id: u16, text: &str) -> Option<Vec<ShapedGlyph>> {
        let face = self.faces.get(id as usize)?;
        let font = HarfFontRef::new(&face.bytes).ok()?;
        let shaper = face
            .shaper_data
            .shaper(&font)
            .instance(face.instance.as_ref())
            .build();
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_cluster_level(BufferClusterLevel::MonotoneCharacters);
        buffer.set_script(script::LATIN);
        buffer.set_direction(harfrust::Direction::LeftToRight);
        buffer.set_language(Language::new("en").unwrap());
        let shaped = shaper.shape(
            buffer,
            harfrust::ShapeOptions::new(), // unscaled: font units
        );
        let positions = shaped.glyph_positions();
        Some(
            shaped
                .glyph_infos()
                .iter()
                .zip(positions)
                .map(|(info, pos)| ShapedGlyph {
                    id: info.glyph_id,
                    x_advance: pos.x_advance as u32,
                    cluster: info.cluster,
                })
                .collect(),
        )
    }
}

/// One face a font file yields: an identity, a location on the
/// file's axes, and the slope and weight it is matched at.
struct Cut {
    name: String,
    style: String,
    attributes: FaceAttributes,
    location: Location,
    variations: Vec<AxisSetting>,
}

/// The faces a font file yields.
///
/// A variable file yields its named instances, which is the file's
/// own statement of the cuts it offers. A static one — or one whose
/// stylesheet already declared what it is — yields a single face at
/// its default location.
fn cuts(font: &skrifa::FontRef, source: &FontSource) -> Vec<Cut> {
    let attributes = font.attributes();
    let default = FaceAttributes {
        italic: attributes.style != SlopeStyle::Normal,
        weight: attributes.weight.value().round().clamp(1.0, 1000.0) as u16,
    };
    let whole_file = |attributes| {
        vec![Cut {
            name: source.name.clone(),
            style: source.style.clone(),
            attributes,
            location: Location::default(),
            variations: Vec::new(),
        }]
    };
    if let Some(declared) = source.declared {
        return whole_file(declared);
    }
    let axes: Vec<_> = font.axes().iter().collect();
    let family = name_string(font, StringId::TYPOGRAPHIC_FAMILY_NAME)
        .or_else(|| name_string(font, StringId::FAMILY_NAME))
        .unwrap_or_else(|| source.name.clone());
    let instances: Vec<Cut> = font
        .named_instances()
        .iter()
        .filter_map(|instance| {
            let style = name_string(font, instance.subfamily_name_id())?;
            let settings: Vec<AxisSetting> = axes
                .iter()
                .zip(instance.user_coords())
                .map(|(axis, value)| AxisSetting {
                    tag: axis.tag().into_bytes(),
                    value,
                })
                .collect();
            let location = instance.location();
            let at_default = location.coords().iter().all(|coord| coord.to_f32() == 0.0);
            Some(Cut {
                name: format!("{family} {style}"),
                attributes: FaceAttributes {
                    italic: default.italic || slanted(&settings),
                    weight: setting(&settings, b"wght")
                        .map(|weight| weight.round().clamp(1.0, 1000.0) as u16)
                        .unwrap_or(default.weight),
                },
                style,
                variations: if at_default { Vec::new() } else { settings },
                location,
            })
        })
        .collect();
    if instances.is_empty() {
        whole_file(default)
    } else {
        instances
    }
}

/// The value of one axis in a face's location.
fn setting(settings: &[AxisSetting], tag: &[u8; 4]) -> Option<f32> {
    settings
        .iter()
        .find(|setting| setting.tag == *tag)
        .map(|setting| setting.value)
}

/// Whether a location leans: a family that varies its slope says so
/// on `ital` or `slnt` rather than in a separate file.
fn slanted(settings: &[AxisSetting]) -> bool {
    setting(settings, b"ital").is_some_and(|value| value >= 0.5)
        || setting(settings, b"slnt").is_some_and(|value| value != 0.0)
}

/// Where a candidate weight sits in CSS's order of preference for a
/// desired one: lower is better, and the tier dominates the distance.
///
/// The rules are asymmetric on purpose — in the text range a heavier
/// cut is preferred to a lighter one, and outside it the search runs
/// away from 400 first.
fn weight_rank(candidate: u16, desired: u16) -> (u8, u16) {
    if (400..=500).contains(&desired) {
        if candidate >= desired && candidate <= 500 {
            (0, candidate - desired)
        } else if candidate < desired {
            (1, desired - candidate)
        } else {
            (2, candidate - 500)
        }
    } else if desired < 400 {
        if candidate <= desired {
            (0, desired - candidate)
        } else {
            (1, candidate - desired)
        }
    } else if candidate >= desired {
        (0, candidate - desired)
    } else {
        (1, desired - candidate)
    }
}

fn name_string<'a>(font: &skrifa::FontRef<'a>, id: StringId) -> Option<String> {
    font.localized_strings(id)
        .english_or_first()
        .map(|s| s.to_string())
}

fn read_metrics(font: &skrifa::FontRef, location: &Location) -> FontMetricsTable {
    let metrics = Metrics::new(font, Size::unscaled(), LocationRef::from(location));
    FontMetricsTable {
        units_per_em: metrics.units_per_em,
        ascender: metrics.ascent as i16,
        descender: metrics.descent as i16,
        line_gap: metrics.leading as i16,
        cap_height: metrics.cap_height.unwrap_or_default() as i16,
    }
}

/// What can go wrong loading a font.
#[derive(Debug, thiserror::Error)]
pub enum FontError {
    /// The bytes are not a font this build can read.
    #[error("font data could not be parsed")]
    Parse,
    /// A font with no family name has nothing to register under.
    #[error("font has no family name")]
    MissingName,
}

/// The bundled upright text face: EB Garamond (SIL OFL 1.1).
pub const BUNDLED_FONT: &[u8] = include_bytes!("../fonts/EBGaramond-VF.ttf");

/// Its italic companion, from the same release: emphasis is a
/// different set of outlines, not a slanted copy of these.
pub const BUNDLED_ITALIC: &[u8] = include_bytes!("../fonts/EBGaramond-Italic-VF.ttf");

/// A registry with the bundled family registered — both slopes, and
/// every weight each file names — and all generics mapped to it.
pub fn bundled_registry() -> Result<FontRegistry, FontError> {
    let mut registry = FontRegistry::new();
    for bytes in [BUNDLED_FONT, BUNDLED_ITALIC] {
        registry.add(FontSource::from_bytes(bytes.to_vec())?)?;
    }
    for generic in [
        GenericFamily::Serif,
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
    ] {
        registry
            .map_generic(generic, "eb garamond")
            .expect("bundled face is registered");
    }
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> FontRegistry {
        bundled_registry().expect("bundled font parses")
    }

    /// Bytes enter, sequential ids come out, and the registry counts
    /// what it holds.
    #[test]
    fn registration_assigns_sequential_ids() {
        let mut registry = FontRegistry::new();
        assert!(registry.is_empty());
        let mut source = FontSource::from_bytes(BUNDLED_FONT.to_vec()).unwrap();
        source.declared = Some(FaceAttributes::REGULAR);
        let id = registry.add(source.clone()).unwrap();
        assert_eq!(id, vec![0]);
        let second = registry.add(source).unwrap();
        assert_eq!(second, vec![1]);
        assert_eq!(registry.len(), 2);
    }

    /// A variable file registers the cuts it names, each pinned to
    /// its own place on the axis. The default instance pins nothing:
    /// there is no instancing to do at the location the file already
    /// sits at.
    #[test]
    fn variable_files_register_their_named_instances() {
        let registry = registry();
        let cuts: Vec<(&str, bool, u16)> = (0..registry.len() as u16)
            .map(|id| {
                let entry = registry.font_ref(id).unwrap();
                (
                    entry.style.as_str(),
                    entry.attributes.italic,
                    entry.attributes.weight,
                )
            })
            .collect();
        assert_eq!(
            cuts,
            vec![
                ("Regular", false, 400),
                ("Medium", false, 500),
                ("SemiBold", false, 600),
                ("Bold", false, 700),
                ("ExtraBold", false, 800),
                ("Italic", true, 400),
                ("Medium Italic", true, 500),
                ("SemiBold Italic", true, 600),
                ("Bold Italic", true, 700),
                ("ExtraBold Italic", true, 800),
            ],
        );
        assert_eq!(registry.variations(0).unwrap(), &[]);
        assert_eq!(
            registry.variations(3).unwrap(),
            &[AxisSetting {
                tag: *b"wght",
                value: 700.0,
            }],
        );
        // The instance is shaped, not just labelled: bold outlines
        // are wider than regular ones at the same size.
        let regular: u32 = registry
            .shape(0, "quantities")
            .unwrap()
            .iter()
            .map(|g| g.x_advance)
            .sum();
        let bold: u32 = registry
            .shape(3, "quantities")
            .unwrap()
            .iter()
            .map(|g| g.x_advance)
            .sum();
        assert!(
            bold > regular,
            "bold ({bold}) is no wider than regular ({regular})"
        );
    }

    /// Slope decides first and weight second: the family has an
    /// italic cut, so an italic request never comes back upright,
    /// and a bold italic request lands on the bold italic instance.
    #[test]
    fn faces_match_by_slope_then_weight() {
        let registry = registry();
        let select = |italic, weight| {
            registry
                .select("EB Garamond", FaceAttributes { italic, weight })
                .unwrap()
        };
        assert_eq!(select(false, 400).id, 0);
        assert_eq!(select(true, 400).id, 5);
        assert_eq!(select(false, 700).id, 3);
        assert_eq!(select(true, 700).id, 8, "no bold italic cut");
        // CSS's desired-weight rules: in the text range the search
        // runs up first, and outside it away from the range.
        assert_eq!(select(false, 450).attributes.weight, 500);
        assert_eq!(select(false, 100).attributes.weight, 400);
        assert_eq!(select(false, 1000).attributes.weight, 800);
        assert_eq!(registry.select("nowhere", FaceAttributes::REGULAR), None);
    }

    /// A family with one slope answers for both, and says which one
    /// it handed back.
    #[test]
    fn a_match_reports_the_face_it_settled_for() {
        let mut registry = FontRegistry::new();
        registry
            .add(FontSource::from_bytes(BUNDLED_FONT.to_vec()).unwrap())
            .unwrap();
        let found = registry
            .select(
                "eb garamond",
                FaceAttributes {
                    italic: true,
                    weight: 400,
                },
            )
            .unwrap();
        assert_eq!(found.id, 0);
        assert!(!found.attributes.italic, "there is no italic cut to find");
    }

    /// A stylesheet that declares what its source is overrides the
    /// file, and registers it as the one cut it was called: a sheet
    /// naming one weight is not describing five.
    #[test]
    fn a_declared_face_registers_as_the_one_cut_it_was_called() {
        let mut registry = FontRegistry::new();
        let mut source = FontSource::from_bytes(BUNDLED_FONT.to_vec()).unwrap();
        source.family = "house".into();
        source.declared = Some(FaceAttributes {
            italic: true,
            weight: 700,
        });
        registry.add(source).unwrap();
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.font_ref(0).unwrap().attributes,
            FaceAttributes {
                italic: true,
                weight: 700,
            },
        );
    }

    /// Metrics come out in font units with the sign convention of the
    /// source tables.
    #[test]
    fn metrics_parse_in_font_units() {
        let registry = registry();
        let metrics = registry.metrics(0).unwrap();
        assert_eq!(metrics.units_per_em, 1000);
        assert_eq!(metrics.ascender, 1007);
        assert_eq!(metrics.descender, -298);
        assert_eq!(metrics.line_gap, 0);
    }

    /// hmtx advances, read straight from the tables.
    #[test]
    fn advance_widths_match_hmtx() {
        let registry = registry();
        // gid 490 = 'o' (495 units), gid 991 = 'f_f_i' (776), gid
        // 2426 = 'space' (200) — cross-checked against ttx.
        let widths = registry.advance_widths(0, &[490, 991, 2426]).unwrap();
        assert_eq!(widths, vec![495, 776, 200]);
        assert_eq!(registry.advance_width(0, 490).unwrap(), 495);
    }

    /// cmap: characters map to nominal glyphs.
    #[test]
    fn charmap_maps_to_nominal_glyphs() {
        let registry = registry();
        assert_eq!(registry.char_glyph(0, 'o'), Some(490));
        assert_eq!(registry.char_glyph(0, 'A'), Some(1));
        assert_eq!(registry.char_glyph(0, '\u{10FFF0}'), None);
    }

    /// Family and name strings come from the name table (typographic
    /// family preferred).
    #[test]
    fn identity_comes_from_the_name_table() {
        let registry = registry();
        let entry = registry.font_ref(0).unwrap();
        assert_eq!(entry.family, "eb garamond");
        assert_eq!(entry.name, "EB Garamond Regular");
        assert_eq!(entry.style, "Regular");
    }

    /// Generic keywords resolve to the bundled face.
    #[test]
    fn generic_families_map_to_bundled_defaults() {
        let registry = registry();
        for generic in [
            GenericFamily::Serif,
            GenericFamily::SansSerif,
            GenericFamily::Monospace,
        ] {
            assert_eq!(registry.generic(generic), Some(0));
        }
        assert_eq!(GenericFamily::parse("SERIF"), Some(GenericFamily::Serif));
        assert_eq!(GenericFamily::parse("fancy"), None);
    }

    /// Family lookup by name finds the face, any case.
    #[test]
    fn families_resolve_case_insensitively() {
        let registry = registry();
        assert_eq!(registry.by_family("EB Garamond"), Some(0));
        assert_eq!(registry.by_family("nope"), None);
    }

    /// Unparseable bytes are rejected at source build, not at first
    /// shape.
    #[test]
    fn garbage_bytes_fail_at_registration() {
        let err = FontSource::from_bytes(vec![0; 64]).unwrap_err();
        assert!(matches!(err, FontError::Parse));
    }

    /// The acceptance run: harfrust output must equal hb-shape's, for
    /// a string exercising liga, kern, and the qu pair.
    #[test]
    fn shaping_matches_hb_shape_reference() {
        let registry = registry();
        let shaped = registry.shape(0, "AVAToffice quantities").unwrap();
        let expected: &[(u32, u32)] = &[
            (1, 552),   // A (kerned)
            (113, 542), // V (kerned)
            (1, 597),   // A (kerned)
            (98, 565),  // T (kerned)
            (490, 495), // o
            (991, 776), // f_f_i (liga)
            (430, 387), // c — kern pulls 'u' left
            (440, 390), // u
            (2426, 200),
            (505, 522),
            (519, 527),
            (415, 399),
            (484, 528),
            (516, 314),
            (462, 245),
            (516, 314),
            (462, 245),
            (440, 390),
            (509, 323),
        ];
        assert_eq!(
            shaped
                .iter()
                .map(|g| (g.id, g.x_advance))
                .collect::<Vec<_>>(),
            expected,
            "harfrust disagrees with hb-shape on the reference string"
        );
    }

    /// Clusters index the shaped string: ligatures carry their first
    /// cluster, and clusters are monotone even where glyphs merge.
    #[test]
    fn clusters_index_the_input_text() {
        let registry = registry();
        let shaped = registry.shape(0, "AVAToffice quantities").unwrap();
        let clusters: Vec<u32> = shaped.iter().map(|g| g.cluster).collect();
        assert_eq!(
            clusters,
            vec![
                0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20
            ],
            "f_f_i (cluster 5) absorbs 'f','f','i' (6, 7) into one glyph"
        );
    }
}
