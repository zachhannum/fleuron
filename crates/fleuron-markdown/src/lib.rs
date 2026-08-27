//! The markdown frontend: source text in, content tree out.
//!
//! [`to_sections`] is the primitive, and it is per-source rather than
//! per-book. One source yields one or more sections, because both
//! directions are ordinary: a novel may arrive as a single file that
//! has to become sixty chapters, or as sixty files that each become
//! one. [`Sections`] is which of those a source is, said out loud.
//!
//! Composing sources into a book is [`assemble`], a step of its own.
//! The caller orders the sources and decides the metadata, so nothing
//! here has to arbitrate between two files that both claim a title.
//!
//! # What the vocabulary cannot hold
//!
//! The content tree is a book's vocabulary: headings, prose,
//! blockquotes, scene breaks, images. Markdown is wider than that.
//! Constructs outside it degrade to prose and say so through the
//! diagnostics channel, with the line and column they were written
//! at. Text is never dropped, because a manuscript that quietly loses
//! a paragraph is worse than one that warns about a table.
//!
//! # Dialects
//!
//! [`Dialect`] is a set of switches, so the departures a host's
//! markdown makes from CommonMark are configuration rather than a
//! second mapping to keep in step with this one.

#![deny(missing_docs)]

mod cache;
mod convert;
mod frontmatter;

pub use cache::{Cache, SourceKey};
pub use frontmatter::{frontmatter, metadata};

use fleuron::Warning;
use fleuron::content::{Book, HeadingLevel, Metadata, Section};

/// How a source is read: where its sections begin, and which
/// markdown it is written in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Options {
    /// Where a new section begins.
    pub sections: Sections,
    /// The departures from CommonMark the source is allowed.
    pub dialect: Dialect,
}

/// Where one source's sections begin.
///
/// Sections are what the fragmenter starts a page on, so this is the
/// decision that sets a book's page count before any styling does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sections {
    /// The whole source is one section: a file per chapter.
    Whole,
    /// A heading at this level or shallower opens one: a file per
    /// book. `AtHeading(H2)` cuts at `#` and `##` alike, so a
    /// manuscript that opens parts with `#` and chapters with `##`
    /// starts a page on both.
    AtHeading(HeadingLevel),
}

impl Default for Sections {
    fn default() -> Sections {
        Sections::AtHeading(HeadingLevel::H1)
    }
}

impl Sections {
    /// Whether a heading at this level opens a section.
    fn opens(self, level: HeadingLevel) -> bool {
        match self {
            Sections::Whole => false,
            Sections::AtHeading(deepest) => u8::from(level) <= u8::from(deepest),
        }
    }
}

/// Which markdown a source is written in.
///
/// Frontmatter is on by default because a manuscript's metadata has
/// to live somewhere, and the alternative is a convention smuggled
/// through the prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dialect {
    /// A leading `---` block is metadata rather than a scene break.
    pub frontmatter: bool,
    /// GitHub's additions: tables, strikethrough, task lists.
    pub gfm: bool,
    /// `[[wikilinks]]`, as Obsidian writes them.
    pub wikilinks: bool,
    /// `--` and `"` become dashes and curly quotes at parse rather
    /// than in the manuscript.
    pub smart_punctuation: bool,
}

impl Default for Dialect {
    fn default() -> Dialect {
        Dialect {
            frontmatter: true,
            gfm: false,
            wikilinks: false,
            smart_punctuation: false,
        }
    }
}

impl Dialect {
    /// CommonMark and nothing else, frontmatter included.
    pub fn common_mark() -> Dialect {
        Dialect::default()
    }

    /// What an Obsidian vault contains.
    pub fn obsidian() -> Dialect {
        Dialect {
            frontmatter: true,
            gfm: true,
            wikilinks: true,
            smart_punctuation: false,
        }
    }

    /// GitHub-flavoured markdown.
    pub fn gfm() -> Dialect {
        Dialect {
            gfm: true,
            ..Dialect::default()
        }
    }
}

/// Reads one source into sections, and everything the reading had to
/// complain about.
///
/// `source` names the file for diagnostics and becomes every
/// section's `source`; it is what [`fleuron::session::Session::replace_source`]
/// replaces by. Node ids are left unassigned: they are assigned in
/// document order over a whole book, which is [`assemble`]'s job.
pub fn to_sections(text: &str, source: &str, options: &Options) -> (Vec<Section>, Vec<Warning>) {
    convert::run(text, source, options)
}

/// Composes ordered sections into a book under the metadata the
/// caller decided on, and numbers the tree.
pub fn assemble(metadata: Metadata, sections: Vec<Section>) -> Book {
    let mut book = Book { metadata, sections };
    book.assign_node_ids();
    book
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_heading_opens_a_section_at_its_level_or_shallower() {
        let at_two = Sections::AtHeading(HeadingLevel::H2);
        assert!(at_two.opens(HeadingLevel::H1));
        assert!(at_two.opens(HeadingLevel::H2));
        assert!(!at_two.opens(HeadingLevel::H3));
        for level in [HeadingLevel::H1, HeadingLevel::H6] {
            assert!(!Sections::Whole.opens(level));
        }
    }

    #[test]
    fn assembly_numbers_the_tree_it_composed() {
        let (first, _) = to_sections("# One\n\nA.\n", "one.md", &Options::default());
        let (second, _) = to_sections("# Two\n\nB.\n", "two.md", &Options::default());
        let book = assemble(
            Metadata::default(),
            first.into_iter().chain(second).collect(),
        );
        assert_eq!(book.sections.len(), 2);
        let ids: Vec<u32> = book.sections.iter().map(|s| s.id.get()).collect();
        assert!(ids[0] > 0 && ids[1] > ids[0], "{ids:?}");
    }

    /// A chapter file's frontmatter is the chapter's. Book metadata
    /// is handed to assembly, so nothing here has to guess which of
    /// sixty files was describing the work.
    #[test]
    fn assembly_takes_the_metadata_it_is_given() {
        let per_file = Options {
            sections: Sections::Whole,
            ..Options::default()
        };
        let mut sections = Vec::new();
        for (name, markdown) in [
            (
                "ch01.md",
                "---\ntitle: The Ambassador\n---\n\nHe arrived.\n",
            ),
            (
                "ch02.md",
                "---\ntitle: A Cold Reception\n---\n\nNobody met him.\n",
            ),
        ] {
            sections.extend(to_sections(markdown, name, &per_file).0);
        }
        let book = assemble(
            metadata("title: The Levant Papers\nauthor: E. Marsh\n"),
            sections,
        );

        assert_eq!(book.metadata.title.as_deref(), Some("The Levant Papers"));
        assert_eq!(book.metadata.author.as_deref(), Some("E. Marsh"));
        let chapters: Vec<Option<&str>> = book
            .sections
            .iter()
            .map(|section| section.title.as_deref())
            .collect();
        assert_eq!(chapters, [Some("The Ambassador"), Some("A Cold Reception")]);
    }
}
