//! The metadata block: a manuscript's title and author, where a
//! manuscript already keeps them.

use fleuron::content::Metadata;

/// Reads the `---` block at the top of a source.
///
/// `title` and `author` are the named fields; every other scalar
/// joins `extra`, which the engine passes through and style may read. Values
/// are scalars. A line that is not `key: value` is not metadata, and
/// nothing outside the block is looked at.
///
/// A source with no leading block has no metadata, which lays out
/// fine.
pub fn frontmatter(text: &str) -> Metadata {
    let mut metadata = Metadata::default();
    let Some(block) = block(text) else {
        return metadata;
    };
    for line in block.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim()).to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "title" => metadata.title = Some(value),
            "author" => metadata.author = Some(value),
            other if !other.is_empty() => {
                metadata.extra.insert(other.to_string(), value);
            }
            _ => {}
        }
    }
    metadata
}

/// The text between the opening `---` and the closing one, when the
/// source opens with a block at all.
fn block(text: &str) -> Option<&str> {
    let body = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    body.split("\n---")
        .next()
        .filter(|front| front.len() < body.len())
}

/// Strips a matching pair of quotes, as YAML would.
fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_fields_are_named_and_the_rest_is_extra() {
        let metadata =
            frontmatter("---\ntitle: A Book\nAuthor: \"Someone\"\nyear: 1900\n---\n\n# One\n");
        assert_eq!(metadata.title.as_deref(), Some("A Book"));
        assert_eq!(metadata.author.as_deref(), Some("Someone"));
        assert_eq!(metadata.extra.get("year").map(String::as_str), Some("1900"));
    }

    #[test]
    fn a_source_without_a_block_has_no_metadata() {
        assert_eq!(frontmatter("# One\n\nProse.\n"), Metadata::default());
        // An opening fence that never closes is prose, not metadata.
        assert_eq!(frontmatter("---\ntitle: A Book\n"), Metadata::default());
    }
}
