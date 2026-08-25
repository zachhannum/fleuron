//! The font registry: font bytes in, `font_id`s out.
//!
//! Both the shaper and every painter resolve glyphs through this one
//! table, which is what makes preview equal export. The registry is
//! the sole owner of font bytes; nothing else in the engine holds a
//! `FontRef` lifetime.

use std::collections::HashMap;

use harfrust::{
    BufferClusterLevel, FontRef as HarfFontRef, Language, ShaperData, UnicodeBuffer, script,
};
use serde::Serialize;
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};
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
        })
    }
}

/// A generic family keyword, as in CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GenericFamily {
    Serif,
    SansSerif,
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
}

/// Font metrics in font units.
///
/// Ascender/descender/line gap follow the OS/2 typographic values
/// when present, hhea otherwise. Descender is negative, per the
/// tables.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct FontMetricsTable {
    pub units_per_em: u16,
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
}

/// One registered face: bytes plus everything decoded from them.
struct Face {
    bytes: Vec<u8>,
    identity: FontRefEntry,
    metrics: FontMetricsTable,
    /// Decoded once, at registration; the shaper reads through this.
    shaper_data: ShaperData,
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

    /// Registers a face and returns its id.
    ///
    /// Decodes metrics eagerly: a font that can't be parsed fails
    /// here, once, rather than at first shape.
    pub fn add(&mut self, source: FontSource) -> Result<u16, FontError> {
        let font = skrifa::FontRef::new(&source.bytes).map_err(|_| FontError::Parse)?;
        let metrics = read_metrics(&font);
        let shaper_data = ShaperData::new(&font);

        let id = self.faces.len() as u16;
        let identity = FontRefEntry {
            family: source.family.clone(),
            name: source.name.clone(),
            style: source.style.clone(),
        };
        self.by_family
            .entry(source.family.clone())
            .or_default()
            .push(id);
        self.faces.push(Face {
            bytes: source.bytes,
            identity,
            metrics,
            shaper_data,
        });
        Ok(id)
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

    /// Identity of a face, for the output font table.
    pub fn font_ref(&self, id: u16) -> Option<&FontRefEntry> {
        self.faces.get(id as usize).map(|face| &face.identity)
    }

    /// Metrics of a face, in font units.
    pub fn metrics(&self, id: u16) -> Option<FontMetricsTable> {
        self.faces.get(id as usize).map(|face| face.metrics)
    }

    /// The raw bytes of a face, for embedding at write time.
    pub fn bytes(&self, id: u16) -> Option<&[u8]> {
        self.faces
            .get(id as usize)
            .map(|face| face.bytes.as_slice())
    }

    /// Advance width of one glyph, in font units.
    pub fn advance_width(&self, id: u16, glyph: u32) -> Option<u16> {
        let face = self.faces.get(id as usize)?;
        let font = HarfFontRef::new(&face.bytes).ok()?;
        let glyph_metrics = GlyphMetrics::new(&font, Size::unscaled(), LocationRef::default());
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
        let glyph_metrics = GlyphMetrics::new(&font, Size::unscaled(), LocationRef::default());
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
        let shaper = face.shaper_data.shaper(&font).build();
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

fn name_string<'a>(font: &skrifa::FontRef<'a>, id: StringId) -> Option<String> {
    font.localized_strings(id)
        .english_or_first()
        .map(|s| s.to_string())
}

fn read_metrics(font: &skrifa::FontRef) -> FontMetricsTable {
    let metrics = Metrics::new(font, Size::unscaled(), LocationRef::default());
    FontMetricsTable {
        units_per_em: metrics.units_per_em,
        ascender: metrics.ascent as i16,
        descender: metrics.descent as i16,
        line_gap: metrics.leading as i16,
    }
}

/// What can go wrong loading a font.
#[derive(Debug, thiserror::Error)]
pub enum FontError {
    #[error("font data could not be parsed")]
    Parse,
    #[error("font has no family name")]
    MissingName,
}

/// The bundled default text face: EB Garamond (SIL OFL 1.1).
pub const BUNDLED_FONT: &[u8] = include_bytes!("../fonts/EBGaramond-VF.ttf");

/// A registry with the bundled face registered and all generics
/// mapped to it.
pub fn bundled_registry() -> Result<FontRegistry, FontError> {
    let mut registry = FontRegistry::new();
    registry.add(FontSource::from_bytes(BUNDLED_FONT.to_vec())?)?;
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
        let id = registry
            .add(FontSource::from_bytes(BUNDLED_FONT.to_vec()).unwrap())
            .unwrap();
        assert_eq!(id, 0);
        let second = registry
            .add(FontSource::from_bytes(BUNDLED_FONT.to_vec()).unwrap())
            .unwrap();
        assert_eq!(second, 1);
        assert_eq!(registry.len(), 2);
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
