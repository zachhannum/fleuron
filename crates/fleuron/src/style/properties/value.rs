//! The simple values: a length, a colour, and the keywords a
//! property takes one of.

use serde::de::{Error as _, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::fonts::{FeatureSetting, GenericFamily};
use crate::pages::{Side, fade};

/// A CSS length, before it is resolved against what it is relative to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// An absolute length, already in points.
    Points(f32),
    /// A multiple of the font size in force.
    Em(f32),
    /// A multiple of the root font size.
    Rem(f32),
    /// A fraction of the reference the property names.
    Percent(f32),
}

impl Length {
    /// The length in points. `relative` is what `em` and percentages
    /// are measured against — the parent font size for `font-size`,
    /// the element's own for everything else.
    pub fn to_points(self, relative: f32, root: f32) -> f32 {
        match self {
            Length::Points(pt) => pt,
            Length::Em(em) => em * relative,
            Length::Rem(rem) => rem * root,
            Length::Percent(percent) => percent / 100.0 * relative,
        }
    }
}

/// `line-height`, before the font size it multiplies is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineHeight {
    /// The font's own idea of leading.
    Normal,
    /// A multiple of the font size.
    Number(f32),
    /// A length, which computes to the multiple it works out as.
    Length(Length),
}

impl LineHeight {
    /// The unitless multiple this computes to at `size`.
    pub fn to_multiple(self, size: f32, root: f32) -> f32 {
        match self {
            LineHeight::Normal => NORMAL_LINE_HEIGHT,
            LineHeight::Number(number) => number,
            LineHeight::Length(length) => {
                if size > 0.0 {
                    length.to_points(size, root) / size
                } else {
                    NORMAL_LINE_HEIGHT
                }
            }
        }
    }
}

/// What `line-height: normal` works out to. The strut takes its
/// ascent and descent from the font; this is the factor over them.
pub(super) const NORMAL_LINE_HEIGHT: f32 = 1.2;

/// A colour: three eight-bit channels and an eight-bit alpha, where
/// 255 is opaque.
///
/// Serialization has two forms, `#rrggbb` where a person reads it
/// and the four bytes on the wire. A colour that is not opaque reads
/// as `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha: 0 is transparent and 255 is opaque.
    pub a: u8,
}

impl Color {
    /// What a page is set in until a rule sets something else.
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    /// An opaque colour from its three channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// A colour from its three channels and its alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// Whether nothing under the colour shows through it.
    pub const fn opaque(self) -> bool {
        self.a == 255
    }

    /// The same colour with its alpha scaled by `opacity`, from 0 to 1.
    pub fn faded(self, opacity: f32) -> Color {
        Color {
            a: fade(self.a, opacity),
            ..self
        }
    }

    /// The colour written the way CSS writes it, for a painter
    /// that takes a string.
    pub fn to_hex(self) -> String {
        let rgb = format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b);
        if self.opaque() {
            rgb
        } else {
            format!("{rgb}{:02x}", self.a)
        }
    }

    /// A colour from `#rrggbb` or `#rrggbbaa`, and `None` for anything
    /// else.
    pub fn from_hex(text: &str) -> Option<Color> {
        let digits = text.strip_prefix('#')?;
        if !matches!(digits.len(), 6 | 8) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        let alpha = if digits.len() == 8 { channel(6)? } else { 255 };
        Some(Color::rgba(channel(0)?, channel(2)?, channel(4)?, alpha))
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_hex())
        } else {
            [self.r, self.g, self.b, self.a].serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Color, D::Error> {
        if deserializer.is_human_readable() {
            let hex = String::deserialize(deserializer)?;
            Color::from_hex(&hex).ok_or_else(|| {
                D::Error::invalid_value(Unexpected::Str(&hex), &"#rrggbb or #rrggbbaa")
            })
        } else {
            let [r, g, b, a] = <[u8; 4]>::deserialize(deserializer)?;
            Ok(Color::rgba(r, g, b, a))
        }
    }
}

/// A font family as a stylesheet names it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// A family name to match against the registry.
    Named(String),
    /// A generic keyword the registry binds to a face.
    Generic(GenericFamily),
}

/// Upright or italic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FontStyle {
    /// `normal`
    Normal,
    /// `italic`, and `oblique` with it.
    Italic,
}

/// How a line's inline content is distributed across the measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    /// `left`
    Left,
    /// `right`
    Right,
    /// `center`
    Center,
    /// `justify`
    Justify,
}

/// What justification opens up to fill the measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextJustify {
    /// `auto`, and `inter-word` with it: the space between words is
    /// the only thing that gives.
    InterWord,
    /// `inter-character`: the space between letters gives too, a
    /// little.
    InterCharacter,
}

/// Which capitals a run is drawn with, from `font-variant-caps`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FontVariantCaps {
    /// `normal`: the letters the text is written in.
    Normal,
    /// `small-caps`: lowercase letters set as small capitals.
    SmallCaps,
}

/// Which ligatures a run is set with, from
/// `font-variant-ligatures`. A group left at `None` keeps whatever
/// the shaper does with it, which is `liga`, `clig` and `calt` on
/// and the rest off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
pub struct FontVariantLigatures {
    /// `common-ligatures` or `no-common-ligatures`: `liga` and
    /// `clig`.
    pub common: Option<bool>,
    /// `discretionary-ligatures` or `no-discretionary-ligatures`:
    /// `dlig`.
    pub discretionary: Option<bool>,
    /// `historical-ligatures` or `no-historical-ligatures`: `hlig`.
    pub historical: Option<bool>,
    /// `contextual` or `no-contextual`: `calt`.
    pub contextual: Option<bool>,
}

impl FontVariantLigatures {
    /// `normal`: every group left to the shaper.
    pub const NORMAL: FontVariantLigatures = FontVariantLigatures {
        common: None,
        discretionary: None,
        historical: None,
        contextual: None,
    };

    /// `none`: every ligature off.
    pub const NONE: FontVariantLigatures = FontVariantLigatures {
        common: Some(false),
        discretionary: Some(false),
        historical: Some(false),
        contextual: Some(false),
    };

    /// The features this value asks the face for.
    pub fn settings(&self) -> Vec<FeatureSetting> {
        let groups: [(Option<bool>, &[&[u8; 4]]); 4] = [
            (self.common, &[b"liga", b"clig"]),
            (self.discretionary, &[b"dlig"]),
            (self.historical, &[b"hlig"]),
            (self.contextual, &[b"calt"]),
        ];
        groups
            .iter()
            .filter_map(|(asked, tags)| Some((asked.as_ref()?, tags)))
            .flat_map(|(on, tags)| {
                tags.iter()
                    .map(move |tag| FeatureSetting::new(**tag, u32::from(*on)))
            })
            .collect()
    }
}

/// Which figures a run is set with, from `font-variant-numeric`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
pub struct FontVariantNumeric {
    /// `lining-nums` or `oldstyle-nums`.
    pub figures: Option<Figures>,
    /// `proportional-nums` or `tabular-nums`.
    pub spacing: Option<NumericSpacing>,
    /// `diagonal-fractions` or `stacked-fractions`.
    pub fractions: Option<Fractions>,
    /// `ordinal`: the letters after a number in `1st`.
    pub ordinal: bool,
    /// `slashed-zero`.
    pub slashed_zero: bool,
}

/// Which shape the figures take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Figures {
    /// `lining-nums`: figures that stand on the baseline at cap
    /// height.
    Lining,
    /// `oldstyle-nums`: figures that rise and fall around it.
    OldStyle,
}

/// How much width each figure takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumericSpacing {
    /// `proportional-nums`: each figure takes the width it draws.
    Proportional,
    /// `tabular-nums`: every figure takes one width, so columns of
    /// them line up.
    Tabular,
}

/// How a fraction is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fractions {
    /// `diagonal-fractions`: the figures stand either side of a
    /// slash.
    Diagonal,
    /// `stacked-fractions`: one figure over the other.
    Stacked,
}

impl FontVariantNumeric {
    /// `normal`: the face's own figures.
    pub const NORMAL: FontVariantNumeric = FontVariantNumeric {
        figures: None,
        spacing: None,
        fractions: None,
        ordinal: false,
        slashed_zero: false,
    };

    /// The features this value asks the face for.
    pub fn settings(&self) -> Vec<FeatureSetting> {
        let tags = [
            self.figures.map(|figures| match figures {
                Figures::Lining => b"lnum",
                Figures::OldStyle => b"onum",
            }),
            self.spacing.map(|spacing| match spacing {
                NumericSpacing::Proportional => b"pnum",
                NumericSpacing::Tabular => b"tnum",
            }),
            self.fractions.map(|fractions| match fractions {
                Fractions::Diagonal => b"frac",
                Fractions::Stacked => b"afrc",
            }),
            self.ordinal.then_some(b"ordn"),
            self.slashed_zero.then_some(b"zero"),
        ];
        tags.into_iter()
            .flatten()
            .map(|tag| FeatureSetting::new(*tag, 1))
            .collect()
    }
}

/// Which alternate glyphs a run is set with, from
/// `font-variant-alternates`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontVariantAlternates {
    /// `normal`: the glyphs the face draws by default.
    #[default]
    Normal,
    /// `historical-forms`: `hist`, such as the long s.
    HistoricalForms,
}

impl FontVariantAlternates {
    /// The features this value asks the face for.
    pub fn settings(&self) -> Vec<FeatureSetting> {
        match self {
            FontVariantAlternates::Normal => Vec::new(),
            FontVariantAlternates::HistoricalForms => {
                vec![FeatureSetting::new(*b"hist", 1)]
            }
        }
    }
}

/// What a run's letters are transformed to before they are shaped,
/// from `text-transform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextTransform {
    /// `none`
    None,
    /// `uppercase`
    Uppercase,
    /// `lowercase`
    Lowercase,
    /// `capitalize`: the first letter of every word.
    Capitalize,
}

impl TextTransform {
    /// Writes one character as this transform spells it, and answers
    /// whether that differs from what was read. Case mapping is not
    /// one for one, since `ß` uppercases to two letters, so what is
    /// written is a stretch of text rather than a character.
    ///
    /// `word_start` is whether the character opens a word, which is
    /// the only thing `capitalize` reads.
    pub fn write(self, letter: char, word_start: bool, out: &mut String) -> bool {
        let at = out.len();
        match self {
            TextTransform::None => out.push(letter),
            TextTransform::Uppercase => out.extend(letter.to_uppercase()),
            TextTransform::Lowercase => out.extend(letter.to_lowercase()),
            TextTransform::Capitalize if word_start => out.extend(letter.to_uppercase()),
            TextTransform::Capitalize => out.push(letter),
        }
        let mut one = [0u8; 4];
        out[at..] != *letter.encode_utf8(&mut one)
    }
}

/// Whether words may be broken at syllable boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hyphens {
    /// `none`, and `manual` with it: only explicit soft hyphens break.
    None,
    /// `auto`
    Auto,
}

/// A fragmentation instruction: the value of `break-before`,
/// `break-after` and `break-inside`. `recto` and `verso` are the book's names
/// for `right` and `left`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Break {
    /// `auto`
    Auto,
    /// `avoid`
    Avoid,
    /// `column`: the flow moves to the next column, and only the last
    /// column of a page moving on ends the page.
    Column,
    /// `page`
    Page,
    /// Break to the next page that falls on the given side, leaving a
    /// blank behind if the flow sits on the wrong one.
    Side(Side),
}

/// What a block's decoration does where a page break splits it, from
/// `box-decoration-break`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BoxDecorationBreak {
    /// `slice`: the box is drawn as one and cut, so the two edges
    /// the break made are open.
    Slice,
    /// `clone`: each piece is a box of its own, closed on all four
    /// edges.
    Clone,
}

/// The size a box is asked to take on one axis, from `width`,
/// `height` or `min-height`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    /// `auto`: the box takes the size that layout gives it.
    Auto,
    /// A length, in points.
    Points(f32),
    /// A percentage of the same axis of the box around it.
    Percent(f32),
}

impl Width {
    /// The size in points inside a box `of` points on the same axis,
    /// or `None` for `auto`.
    pub fn resolve(self, of: f32) -> Option<f32> {
        match self {
            Width::Auto => None,
            Width::Points(points) => Some(points),
            Width::Percent(percent) => Some(percent / 100.0 * of),
        }
    }
}

/// Whether the cells of a table share their borders, from
/// `border-collapse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderCollapse {
    /// `separate`: every cell draws its own border, beside the border
    /// of the cell next to it.
    Separate,
    /// `collapse`: two cells that meet draw one border between them.
    Collapse,
}

/// How many columns of a divided page a block is set across, from
/// `column-span`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnSpan {
    /// `none`: the block is set in one column.
    None,
    /// `all`: the block is set across the whole content box, with
    /// columns above it and columns below it.
    All,
}

/// Which rules a run has drawn across it, from
/// `text-decoration-line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
pub struct DecorationLine {
    /// `underline`: a rule under the text.
    pub under: bool,
    /// `overline`: a rule over it.
    pub over: bool,
    /// `line-through`: a rule across it.
    pub through: bool,
}

impl DecorationLine {
    /// No rule at all, which is `none`.
    pub const NONE: DecorationLine = DecorationLine {
        under: false,
        over: false,
        through: false,
    };

    /// Whether any rule is drawn.
    pub fn draws(self) -> bool {
        self.under || self.over || self.through
    }
}

/// How one rule is drawn, from `text-decoration-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecorationStyle {
    /// `solid`: one rule.
    #[default]
    Solid,
    /// `double`: two rules, one thickness apart.
    Double,
}

impl DecorationStyle {
    /// How many rules one decoration draws.
    pub fn rules(self) -> u8 {
        match self {
            DecorationStyle::Solid => 1,
            DecorationStyle::Double => 2,
        }
    }
}

/// What a run has drawn across it: the rules, what they are painted
/// in, how they are drawn, and how thick they are.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct TextDecoration {
    /// Which rules are drawn.
    pub line: DecorationLine,
    /// What they are painted in. `None` is the colour of the text.
    pub color: Option<Color>,
    /// How each one is drawn.
    pub style: DecorationStyle,
    /// How thick each one is, in points. `None` is the thickness the
    /// face declares.
    pub thickness: Option<f32>,
}

impl TextDecoration {
    /// Nothing drawn, which is what a run takes until a rule asks
    /// for something.
    pub const NONE: TextDecoration = TextDecoration {
        line: DecorationLine::NONE,
        color: None,
        style: DecorationStyle::Solid,
        thickness: None,
    };

    /// Whether anything is drawn.
    pub fn draws(&self) -> bool {
        self.line.draws()
    }
}
