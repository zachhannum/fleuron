//! Text made safe to write into XML.

/// `text` as character data.
pub fn text(out: &mut String, text: &str) {
    for c in text.chars().filter(|c| allowed(*c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// `value` as the value of an attribute written between double
/// quotes.
pub fn attribute(out: &mut String, value: &str) {
    for c in value.chars().filter(|c| allowed(*c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            // A literal tab or newline in an attribute reads back as a
            // space.
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}

/// Whether XML 1.0 can hold the character at all.
fn allowed(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_characters_are_escaped_and_control_characters_dropped() {
        let mut out = String::new();
        text(&mut out, "a < b & c > d\u{7}");
        assert_eq!(out, "a &lt; b &amp; c &gt; d");
        let mut out = String::new();
        attribute(&mut out, "say \"hi\"\n");
        assert_eq!(out, "say &quot;hi&quot;&#10;");
    }
}
