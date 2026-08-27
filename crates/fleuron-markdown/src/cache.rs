//! Parsed sections, kept between edits.
//!
//! A host that re-renders on every keystroke re-reads one file and
//! holds the rest. The key is the source's name and a hash of its
//! bytes, and it deliberately is not a node id: ids are assigned in
//! document order over the whole book and renumber whenever a section
//! is added or removed, so a cache keyed on one would miss on every
//! edit while appearing to work.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use fleuron::Warning;
use fleuron::content::Section;

use crate::{Options, to_sections};

/// What identifies one parse: the source's name and its content.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceKey {
    /// The file the sections came from.
    pub source: String,
    /// A hash of the source text and the options it was read under.
    pub content: u64,
}

impl SourceKey {
    /// The key one reading of a source would be stored under.
    pub fn of(text: &str, source: &str, options: &Options) -> SourceKey {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        options.sections.hash(&mut hasher);
        options.dialect.hash(&mut hasher);
        SourceKey {
            source: source.to_string(),
            content: hasher.finish(),
        }
    }
}

/// Sections held by source, re-read only when the source changed.
#[derive(Debug, Default)]
pub struct Cache {
    entries: HashMap<String, (u64, Vec<Section>, Vec<Warning>)>,
    hits: u32,
}

impl Cache {
    /// An empty cache.
    pub fn new() -> Cache {
        Cache::default()
    }

    /// The source's sections, read again only if its bytes moved.
    ///
    /// The sections come back by value because that is what assembly
    /// and [`fleuron::session::Session::replace_source`] both want;
    /// what the cache saves is the reading, not the copy.
    pub fn to_sections(
        &mut self,
        text: &str,
        source: &str,
        options: &Options,
    ) -> (Vec<Section>, Vec<Warning>) {
        let key = SourceKey::of(text, source, options);
        if let Some((content, sections, warnings)) = self.entries.get(&key.source)
            && *content == key.content
        {
            self.hits += 1;
            return (sections.clone(), warnings.clone());
        }
        let (sections, warnings) = to_sections(text, source, options);
        self.entries.insert(
            key.source,
            (key.content, sections.clone(), warnings.clone()),
        );
        (sections, warnings)
    }

    /// Forgets one source, for a file the host no longer holds.
    pub fn forget(&mut self, source: &str) {
        self.entries.remove(source);
    }

    /// How many readings the cache has answered without parsing.
    pub fn hits(&self) -> u32 {
        self.hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_read_again_only_when_its_bytes_move() {
        let mut cache = Cache::new();
        let options = Options::default();
        let first = cache.to_sections("# One\n\nA.\n", "one.md", &options).0;
        assert_eq!(cache.hits(), 0);

        let again = cache.to_sections("# One\n\nA.\n", "one.md", &options).0;
        assert_eq!(cache.hits(), 1);
        assert_eq!(first, again);

        let edited = cache.to_sections("# One\n\nB.\n", "one.md", &options).0;
        assert_eq!(cache.hits(), 1, "an edit is not a hit");
        assert_ne!(first, edited);
    }

    /// Two sources with the same bytes are two entries: the name is
    /// half the key, and it is what a section carries.
    #[test]
    fn the_name_is_part_of_the_key() {
        let mut cache = Cache::new();
        let options = Options::default();
        let one = cache.to_sections("# One\n\nA.\n", "one.md", &options).0;
        let two = cache.to_sections("# One\n\nA.\n", "two.md", &options).0;
        assert_eq!(cache.hits(), 0);
        assert_ne!(one[0].source, two[0].source);
    }
}
