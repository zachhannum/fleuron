//! What a margin box holds: a counter spelled out, a named string,
//! or text the sheet wrote.

use serde::Serialize;

/// What a page margin box paints.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Content {
    /// Nothing: the box is not generated.
    None,
    /// The page's own folio, spelled as the style names.
    Counter(CounterStyle),
    /// A running string, at the value it stood at when the page began.
    /// A string nothing has set yet paints nothing.
    String(String),
    /// A literal string.
    Text(String),
}

/// How a counter's value is spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CounterStyle {
    /// `1`, `2`, `3`
    Decimal,
    /// `i`, `ii`, `iii`
    LowerRoman,
    /// `I`, `II`, `III`
    UpperRoman,
    /// `a`, `b`, `c`
    LowerAlpha,
    /// `A`, `B`, `C`
    UpperAlpha,
}

impl CounterStyle {
    /// Every style, in the order the subset lists them.
    pub const ALL: [CounterStyle; 5] = [
        CounterStyle::Decimal,
        CounterStyle::LowerRoman,
        CounterStyle::UpperRoman,
        CounterStyle::LowerAlpha,
        CounterStyle::UpperAlpha,
    ];

    /// The CSS keyword.
    pub fn keyword(self) -> &'static str {
        match self {
            CounterStyle::Decimal => "decimal",
            CounterStyle::LowerRoman => "lower-roman",
            CounterStyle::UpperRoman => "upper-roman",
            CounterStyle::LowerAlpha => "lower-alpha",
            CounterStyle::UpperAlpha => "upper-alpha",
        }
    }

    /// Parses one of the keywords, or `None` for a style outside the
    /// subset.
    pub fn parse(keyword: &str) -> Option<CounterStyle> {
        CounterStyle::ALL
            .into_iter()
            .find(|style| style.keyword().eq_ignore_ascii_case(keyword))
    }

    /// One value as this style spells it. A value the style has no
    /// spelling for — nothing before `i`, nothing past `mmmcmxcix` —
    /// falls back to decimal, as CSS asks.
    pub fn format(self, value: u32) -> String {
        match self {
            CounterStyle::Decimal => value.to_string(),
            CounterStyle::LowerRoman => roman(value).unwrap_or_else(|| value.to_string()),
            CounterStyle::UpperRoman => roman(value)
                .map(|numeral| numeral.to_uppercase())
                .unwrap_or_else(|| value.to_string()),
            CounterStyle::LowerAlpha => alpha(value).unwrap_or_else(|| value.to_string()),
            CounterStyle::UpperAlpha => alpha(value)
                .map(|letters| letters.to_uppercase())
                .unwrap_or_else(|| value.to_string()),
        }
    }
}

/// Roman numerals, lowercase, over the range they have spellings for.
fn roman(value: u32) -> Option<String> {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    if !(1..4000).contains(&value) {
        return None;
    }
    let mut left = value;
    let mut numeral = String::new();
    for (amount, digits) in NUMERALS {
        while left >= amount {
            numeral.push_str(digits);
            left -= amount;
        }
    }
    Some(numeral)
}

/// Bijective base 26, lowercase: `a`…`z`, `aa`…
fn alpha(value: u32) -> Option<String> {
    if value == 0 {
        return None;
    }
    let mut left = value;
    let mut letters = Vec::new();
    while left > 0 {
        let digit = (left - 1) % 26;
        letters.push(b'a' + digit as u8);
        left = (left - 1) / 26;
    }
    letters.reverse();
    Some(String::from_utf8(letters).expect("ascii letters"))
}

/// One `string-set` entry: a named string, and what the element sets
/// it to when the flow reaches it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct StringSet {
    /// The string's name, as `string()` asks for it.
    pub name: String,
    /// The pieces the value is built from, concatenated.
    pub value: Vec<StringPiece>,
}

/// One piece of a `string-set` value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StringPiece {
    /// `content()`: the element's own text.
    Content,
    /// A literal.
    Text(String),
}
