//! Colour values: the hex, keyword and function forms, and the
//! named colours behind the keyword.

use cssparser::{ParseError, Parser, Token};

use crate::style::properties::Color;

use super::StyleError;

/// `color: <named-colour> | #rgb | #rrggbb | rgb()`.
pub(super) fn color(input: &mut Parser<'_, '_>) -> Option<Color> {
    if let Ok(color) = input.try_parse(rgb_function) {
        return Some(color);
    }
    match input.next().ok()? {
        Token::Hash(digits) | Token::IDHash(digits) => hex(digits),
        Token::Ident(name) => named(name),
        _ => None,
    }
}

/// `rgb(r, g, b)`, with either commas or spaces between the
/// channels. All three are numbers or all three are percentages.
pub(super) fn rgb_function<'i>(
    input: &mut Parser<'i, '_>,
) -> Result<Color, ParseError<'i, StyleError<'i>>> {
    input.expect_function_matching("rgb")?;
    input.parse_nested_block(|input| {
        let (red, percentages) = channel(input, None)?;
        let commas = input.try_parse(|input| input.expect_comma()).is_ok();
        let (green, _) = channel(input, Some(percentages))?;
        if commas {
            input.expect_comma()?;
        }
        let (blue, _) = channel(input, Some(percentages))?;
        input.expect_exhausted()?;
        Ok(Color::rgb(red, green, blue))
    })
}

/// One channel of `rgb()`: `0` to `255`, or a percentage of it, and
/// which of the two it was. `written` is how the channels before it
/// were written, and a channel that disagrees is not a colour.
pub(super) fn channel<'i>(
    input: &mut Parser<'i, '_>,
    written: Option<bool>,
) -> Result<(u8, bool), ParseError<'i, StyleError<'i>>> {
    let percentage = input.try_parse(|input| input.expect_percentage());
    if written.is_some_and(|percentages| percentages != percentage.is_ok()) {
        return Err(input.new_error_for_next_token());
    }
    let value = match percentage {
        Ok(percentage) => percentage * 255.0,
        Err(_) => input.expect_number()?,
    };
    Ok((value.round().clamp(0.0, 255.0) as u8, percentage.is_ok()))
}

/// The two hex forms. `#abc` is `#aabbcc`: each digit stands for a
/// pair of itself.
pub(super) fn hex(digits: &str) -> Option<Color> {
    let spelled: String = match digits.len() {
        3 => digits.chars().flat_map(|digit| [digit, digit]).collect(),
        6 => digits.to_string(),
        _ => return None,
    };
    Color::from_hex(&format!("#{spelled}"))
}

/// One of the CSS colour names, whatever case it was written in.
pub(super) fn named(name: &str) -> Option<Color> {
    let lowercase = name.to_ascii_lowercase();
    let at = NAMED
        .binary_search_by_key(&lowercase.as_str(), |(known, _)| known)
        .ok()?;
    Some(NAMED[at].1)
}

/// `background-color: <color> | transparent`, where `transparent` is
/// the initial value: nothing is painted behind the block.
pub(super) fn background_color(input: &mut Parser<'_, '_>) -> Option<Option<Color>> {
    if input
        .try_parse(|input| input.expect_ident_matching("transparent"))
        .is_ok()
    {
        return Some(None);
    }
    color(input).map(Some)
}

/// A colour written where a longhand takes one, which never means
/// `currentColor`: only the shorthand leaves the colour out.
pub(super) fn border_color(input: &mut Parser<'_, '_>) -> Option<Option<Color>> {
    color(input).map(Some)
}

/// The CSS named colours, sorted for binary search.
pub(crate) const NAMED: [(&str, Color); 148] = [
    ("aliceblue", Color::rgb(240, 248, 255)),
    ("antiquewhite", Color::rgb(250, 235, 215)),
    ("aqua", Color::rgb(0, 255, 255)),
    ("aquamarine", Color::rgb(127, 255, 212)),
    ("azure", Color::rgb(240, 255, 255)),
    ("beige", Color::rgb(245, 245, 220)),
    ("bisque", Color::rgb(255, 228, 196)),
    ("black", Color::rgb(0, 0, 0)),
    ("blanchedalmond", Color::rgb(255, 235, 205)),
    ("blue", Color::rgb(0, 0, 255)),
    ("blueviolet", Color::rgb(138, 43, 226)),
    ("brown", Color::rgb(165, 42, 42)),
    ("burlywood", Color::rgb(222, 184, 135)),
    ("cadetblue", Color::rgb(95, 158, 160)),
    ("chartreuse", Color::rgb(127, 255, 0)),
    ("chocolate", Color::rgb(210, 105, 30)),
    ("coral", Color::rgb(255, 127, 80)),
    ("cornflowerblue", Color::rgb(100, 149, 237)),
    ("cornsilk", Color::rgb(255, 248, 220)),
    ("crimson", Color::rgb(220, 20, 60)),
    ("cyan", Color::rgb(0, 255, 255)),
    ("darkblue", Color::rgb(0, 0, 139)),
    ("darkcyan", Color::rgb(0, 139, 139)),
    ("darkgoldenrod", Color::rgb(184, 134, 11)),
    ("darkgray", Color::rgb(169, 169, 169)),
    ("darkgreen", Color::rgb(0, 100, 0)),
    ("darkgrey", Color::rgb(169, 169, 169)),
    ("darkkhaki", Color::rgb(189, 183, 107)),
    ("darkmagenta", Color::rgb(139, 0, 139)),
    ("darkolivegreen", Color::rgb(85, 107, 47)),
    ("darkorange", Color::rgb(255, 140, 0)),
    ("darkorchid", Color::rgb(153, 50, 204)),
    ("darkred", Color::rgb(139, 0, 0)),
    ("darksalmon", Color::rgb(233, 150, 122)),
    ("darkseagreen", Color::rgb(143, 188, 143)),
    ("darkslateblue", Color::rgb(72, 61, 139)),
    ("darkslategray", Color::rgb(47, 79, 79)),
    ("darkslategrey", Color::rgb(47, 79, 79)),
    ("darkturquoise", Color::rgb(0, 206, 209)),
    ("darkviolet", Color::rgb(148, 0, 211)),
    ("deeppink", Color::rgb(255, 20, 147)),
    ("deepskyblue", Color::rgb(0, 191, 255)),
    ("dimgray", Color::rgb(105, 105, 105)),
    ("dimgrey", Color::rgb(105, 105, 105)),
    ("dodgerblue", Color::rgb(30, 144, 255)),
    ("firebrick", Color::rgb(178, 34, 34)),
    ("floralwhite", Color::rgb(255, 250, 240)),
    ("forestgreen", Color::rgb(34, 139, 34)),
    ("fuchsia", Color::rgb(255, 0, 255)),
    ("gainsboro", Color::rgb(220, 220, 220)),
    ("ghostwhite", Color::rgb(248, 248, 255)),
    ("gold", Color::rgb(255, 215, 0)),
    ("goldenrod", Color::rgb(218, 165, 32)),
    ("gray", Color::rgb(128, 128, 128)),
    ("green", Color::rgb(0, 128, 0)),
    ("greenyellow", Color::rgb(173, 255, 47)),
    ("grey", Color::rgb(128, 128, 128)),
    ("honeydew", Color::rgb(240, 255, 240)),
    ("hotpink", Color::rgb(255, 105, 180)),
    ("indianred", Color::rgb(205, 92, 92)),
    ("indigo", Color::rgb(75, 0, 130)),
    ("ivory", Color::rgb(255, 255, 240)),
    ("khaki", Color::rgb(240, 230, 140)),
    ("lavender", Color::rgb(230, 230, 250)),
    ("lavenderblush", Color::rgb(255, 240, 245)),
    ("lawngreen", Color::rgb(124, 252, 0)),
    ("lemonchiffon", Color::rgb(255, 250, 205)),
    ("lightblue", Color::rgb(173, 216, 230)),
    ("lightcoral", Color::rgb(240, 128, 128)),
    ("lightcyan", Color::rgb(224, 255, 255)),
    ("lightgoldenrodyellow", Color::rgb(250, 250, 210)),
    ("lightgray", Color::rgb(211, 211, 211)),
    ("lightgreen", Color::rgb(144, 238, 144)),
    ("lightgrey", Color::rgb(211, 211, 211)),
    ("lightpink", Color::rgb(255, 182, 193)),
    ("lightsalmon", Color::rgb(255, 160, 122)),
    ("lightseagreen", Color::rgb(32, 178, 170)),
    ("lightskyblue", Color::rgb(135, 206, 250)),
    ("lightslategray", Color::rgb(119, 136, 153)),
    ("lightslategrey", Color::rgb(119, 136, 153)),
    ("lightsteelblue", Color::rgb(176, 196, 222)),
    ("lightyellow", Color::rgb(255, 255, 224)),
    ("lime", Color::rgb(0, 255, 0)),
    ("limegreen", Color::rgb(50, 205, 50)),
    ("linen", Color::rgb(250, 240, 230)),
    ("magenta", Color::rgb(255, 0, 255)),
    ("maroon", Color::rgb(128, 0, 0)),
    ("mediumaquamarine", Color::rgb(102, 205, 170)),
    ("mediumblue", Color::rgb(0, 0, 205)),
    ("mediumorchid", Color::rgb(186, 85, 211)),
    ("mediumpurple", Color::rgb(147, 112, 219)),
    ("mediumseagreen", Color::rgb(60, 179, 113)),
    ("mediumslateblue", Color::rgb(123, 104, 238)),
    ("mediumspringgreen", Color::rgb(0, 250, 154)),
    ("mediumturquoise", Color::rgb(72, 209, 204)),
    ("mediumvioletred", Color::rgb(199, 21, 133)),
    ("midnightblue", Color::rgb(25, 25, 112)),
    ("mintcream", Color::rgb(245, 255, 250)),
    ("mistyrose", Color::rgb(255, 228, 225)),
    ("moccasin", Color::rgb(255, 228, 181)),
    ("navajowhite", Color::rgb(255, 222, 173)),
    ("navy", Color::rgb(0, 0, 128)),
    ("oldlace", Color::rgb(253, 245, 230)),
    ("olive", Color::rgb(128, 128, 0)),
    ("olivedrab", Color::rgb(107, 142, 35)),
    ("orange", Color::rgb(255, 165, 0)),
    ("orangered", Color::rgb(255, 69, 0)),
    ("orchid", Color::rgb(218, 112, 214)),
    ("palegoldenrod", Color::rgb(238, 232, 170)),
    ("palegreen", Color::rgb(152, 251, 152)),
    ("paleturquoise", Color::rgb(175, 238, 238)),
    ("palevioletred", Color::rgb(219, 112, 147)),
    ("papayawhip", Color::rgb(255, 239, 213)),
    ("peachpuff", Color::rgb(255, 218, 185)),
    ("peru", Color::rgb(205, 133, 63)),
    ("pink", Color::rgb(255, 192, 203)),
    ("plum", Color::rgb(221, 160, 221)),
    ("powderblue", Color::rgb(176, 224, 230)),
    ("purple", Color::rgb(128, 0, 128)),
    ("rebeccapurple", Color::rgb(102, 51, 153)),
    ("red", Color::rgb(255, 0, 0)),
    ("rosybrown", Color::rgb(188, 143, 143)),
    ("royalblue", Color::rgb(65, 105, 225)),
    ("saddlebrown", Color::rgb(139, 69, 19)),
    ("salmon", Color::rgb(250, 128, 114)),
    ("sandybrown", Color::rgb(244, 164, 96)),
    ("seagreen", Color::rgb(46, 139, 87)),
    ("seashell", Color::rgb(255, 245, 238)),
    ("sienna", Color::rgb(160, 82, 45)),
    ("silver", Color::rgb(192, 192, 192)),
    ("skyblue", Color::rgb(135, 206, 235)),
    ("slateblue", Color::rgb(106, 90, 205)),
    ("slategray", Color::rgb(112, 128, 144)),
    ("slategrey", Color::rgb(112, 128, 144)),
    ("snow", Color::rgb(255, 250, 250)),
    ("springgreen", Color::rgb(0, 255, 127)),
    ("steelblue", Color::rgb(70, 130, 180)),
    ("tan", Color::rgb(210, 180, 140)),
    ("teal", Color::rgb(0, 128, 128)),
    ("thistle", Color::rgb(216, 191, 216)),
    ("tomato", Color::rgb(255, 99, 71)),
    ("turquoise", Color::rgb(64, 224, 208)),
    ("violet", Color::rgb(238, 130, 238)),
    ("wheat", Color::rgb(245, 222, 179)),
    ("white", Color::rgb(255, 255, 255)),
    ("whitesmoke", Color::rgb(245, 245, 245)),
    ("yellow", Color::rgb(255, 255, 0)),
    ("yellowgreen", Color::rgb(154, 205, 50)),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::properties::Color;

    /// The name table is sorted, because the search over it is
    /// binary, and every name in it reads back.
    #[test]
    fn every_colour_name_is_in_order_and_reads_back() {
        for pair in NAMED.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
        for (name, color) in NAMED {
            assert_eq!(named(name), Some(color), "{name}");
        }
        assert_eq!(named("REBECCAPURPLE"), Some(Color::rgb(102, 51, 153)));
        assert_eq!(named("octarine"), None);
    }
}
