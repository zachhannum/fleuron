//! The simple values: a length, a colour, and the keywords a
//! property takes one of.

use serde::de::{Error as _, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::fonts::GenericFamily;
use crate::pages::Side;

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

/// A colour: three eight-bit channels and no alpha.
///
/// Serialization has two forms, `#rrggbb` where a person reads it
/// and the three bytes on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
}

impl Color {
    /// What a page is set in until a rule sets something else.
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    /// A colour from its three channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b }
    }

    /// The colour written the way CSS writes it, for a painter
    /// that takes a string.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// A colour from `#rrggbb`, and `None` for anything else.
    pub fn from_hex(text: &str) -> Option<Color> {
        let digits = text.strip_prefix('#')?;
        if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
        Some(Color::rgb(channel(0)?, channel(2)?, channel(4)?))
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(&self.to_hex())
        } else {
            [self.r, self.g, self.b].serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Color, D::Error> {
        if deserializer.is_human_readable() {
            let hex = String::deserialize(deserializer)?;
            Color::from_hex(&hex)
                .ok_or_else(|| D::Error::invalid_value(Unexpected::Str(&hex), &"#rrggbb"))
        } else {
            let [r, g, b] = <[u8; 3]>::deserialize(deserializer)?;
            Ok(Color::rgb(r, g, b))
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

/// The width a box is asked to take, from `width`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Width {
    /// `auto`: the box takes the width that layout gives it.
    Auto,
    /// A length, in points.
    Points(f32),
    /// A percentage of the width of the box around it.
    Percent(f32),
}

impl Width {
    /// The width in points inside a box `of` points wide, or `None`
    /// for `auto`.
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
