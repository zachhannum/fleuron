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
//! # What the vocabulary cannot express
//!
//! The content tree is a book's vocabulary: headings, prose,
//! blockquotes, scene breaks, images, tables. Markdown is wider than
//! that. Constructs outside it degrade to prose and say so through
//! the diagnostics channel, with the line and column they were
//! written at. Text is never dropped, because a manuscript that
//! quietly loses a paragraph is worse than one that warns about a
//! list.
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
pub use frontmatter::frontmatter;

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
/// [`Dialect::fleuron`] is the default. The other three are named
/// after whose markdown they read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dialect {
    /// A leading `---` block is metadata rather than a scene break.
    pub frontmatter: bool,
    /// A line of nothing but `{.class #id}` names the block under it,
    /// and a heading or an image takes the same run written after it.
    pub attributes: bool,
    /// Tables, written as GitHub writes them: a header row, a
    /// delimiter row, and one line per row.
    pub tables: bool,
    /// GitHub's other additions: strikethrough and task lists.
    pub gfm: bool,
    /// `[[wikilinks]]`, as Obsidian writes them.
    pub wikilinks: bool,
    /// `--` and `"` become dashes and curly quotes at parse rather
    /// than in the manuscript.
    pub smart_punctuation: bool,
}

impl Default for Dialect {
    fn default() -> Dialect {
        Dialect::fleuron()
    }
}

impl Dialect {
    /// CommonMark, a frontmatter block, attribute lines and tables:
    /// the markdown a manuscript for this engine is written in, and
    /// what a source is read as unless the host says otherwise.
    pub fn fleuron() -> Dialect {
        Dialect {
            frontmatter: true,
            attributes: true,
            tables: true,
            gfm: false,
            wikilinks: false,
            smart_punctuation: false,
        }
    }

    /// CommonMark, frontmatter included, and nothing besides. A brace
    /// run and a table are prose here.
    pub fn common_mark() -> Dialect {
        Dialect {
            attributes: false,
            tables: false,
            ..Dialect::fleuron()
        }
    }

    /// What an Obsidian vault contains.
    pub fn obsidian() -> Dialect {
        Dialect {
            gfm: true,
            wikilinks: true,
            ..Dialect::fleuron()
        }
    }

    /// GitHub-flavoured markdown.
    pub fn gfm() -> Dialect {
        Dialect {
            gfm: true,
            ..Dialect::fleuron()
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
///
/// The metadata is an argument because a book of many files has no
/// one file to read it from. Each chapter's frontmatter stays with
/// the chapter; the work is named here.
///
/// ```
/// use fleuron::content::Metadata;
/// use fleuron_markdown::{Options, Sections, assemble, to_sections};
///
/// // A file per chapter, so nothing is cut at a heading.
/// let reading = Options {
///     sections: Sections::Whole,
///     ..Options::default()
/// };
/// let chapters = [
///     ("ch01.md", "---\ntitle: The Ambassador\n---\n\nHe arrived.\n"),
///     ("ch02.md", "---\ntitle: A Cold Reception\n---\n\nNobody met him.\n"),
/// ];
///
/// let mut sections = Vec::new();
/// for (name, text) in chapters {
///     sections.extend(to_sections(text, name, &reading).0);
/// }
///
/// let book = assemble(
///     Metadata {
///         title: Some("The Levant Papers".into()),
///         ..Metadata::default()
///     },
///     sections,
/// );
///
/// assert_eq!(book.metadata.title.as_deref(), Some("The Levant Papers"));
/// assert_eq!(book.sections[0].title.as_deref(), Some("The Ambassador"));
/// assert_eq!(book.sections[1].title.as_deref(), Some("A Cold Reception"));
/// ```
pub fn assemble(metadata: Metadata, sections: Vec<Section>) -> Book {
    let mut book = Book { metadata, sections };
    book.assign_node_ids();
    book
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleuron::content::{Block, Inline, LinkTarget};

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

    /// Each source read whole as an Obsidian vault holds it, composed
    /// in the order given, and what the frontend complained about.
    fn composed(sources: &[(&str, &str)]) -> (Book, Vec<Warning>) {
        let vault = Options {
            sections: Sections::Whole,
            dialect: Dialect::obsidian(),
        };
        let mut sections = Vec::new();
        let mut warnings = Vec::new();
        for (name, text) in sources {
            let (read, complaints) = to_sections(text, name, &vault);
            sections.extend(read);
            warnings.extend(complaints);
        }
        (assemble(Metadata::default(), sections), warnings)
    }

    /// The `nth` heading read from `source`, counted from 0.
    fn heading_in(book: &Book, source: &str, nth: usize) -> LinkTarget {
        let node = book
            .sections
            .iter()
            .filter(|section| section.source.as_deref() == Some(source))
            .flat_map(|section| &section.blocks)
            .filter_map(|block| match block {
                Block::Heading { id, .. } => Some(*id),
                _ => None,
            })
            .nth(nth)
            .expect("the source has the heading");
        LinkTarget::Node(node)
    }

    /// The url of every link in the paragraphs read from `source`.
    fn urls(book: &Book, source: &str) -> Vec<String> {
        book.sections
            .iter()
            .filter(|section| section.source.as_deref() == Some(source))
            .flat_map(|section| &section.blocks)
            .filter_map(|block| match block {
                Block::Paragraph { inlines, .. } => Some(inlines),
                _ => None,
            })
            .flatten()
            .filter_map(|inline| match inline {
                Inline::Link { url, .. } => Some(url.clone()),
                _ => None,
            })
            .collect()
    }

    /// Acceptance: a markdown link and a wikilink to a heading in
    /// another note reach it, in each form Obsidian writes.
    #[test]
    fn each_link_form_reaches_the_heading_in_its_file() {
        let (book, warnings) = composed(&[
            (
                "Chapter 1.md",
                "# The Voyage\n\n[a](chapter-03.md#the-hunter)\n[b](Chapter%203.md#The%20Hunter)\n\
                 [[Chapter 3#The Hunter]]\n[[Chapter 3#The Hunter|the hunter]]\n",
            ),
            ("Chapter 3.md", "# The Hunter\n\nHe waited.\n"),
            ("chapter-03.md", "# The Hunter\n\nHe waited again.\n"),
        ]);
        assert!(warnings.is_empty(), "{warnings:?}");
        let anchors = book.anchors();
        let urls = urls(&book, "Chapter 1.md");
        let reached: Vec<LinkTarget> = urls
            .iter()
            .map(|url| anchors.resolve(url, Some("Chapter 1.md")))
            .collect();
        let (spaced, slugged) = (
            heading_in(&book, "Chapter 3.md", 0),
            heading_in(&book, "chapter-03.md", 0),
        );
        assert_eq!(reached, [slugged, spaced, spaced, spaced], "{urls:?}");
    }

    /// Acceptance: a link with only `#heading` reaches the heading in
    /// the source it is written in, so two chapters can each have one
    /// called Notes.
    #[test]
    fn a_link_with_no_file_reaches_its_own_source() {
        let chapter = "# Chapter\n\nSee [the notes](#notes).\n\n# Notes\n";
        let (book, warnings) = composed(&[("one.md", chapter), ("two.md", chapter)]);
        assert!(warnings.is_empty(), "{warnings:?}");
        let anchors = book.anchors();
        for source in ["one.md", "two.md"] {
            assert_eq!(
                anchors.resolve("#notes", Some(source)),
                heading_in(&book, source, 1),
            );
        }
    }

    /// Acceptance: two headings with the same text in one source take
    /// `x` and `x-2`. The text form reaches the first, as in Obsidian.
    #[test]
    fn slugs_count_within_one_source() {
        let (book, _) = composed(&[("one.md", "# Chapter One\n\n# Chapter One\n")]);
        let anchors = book.anchors();
        let at = |url| anchors.resolve(url, Some("one.md"));
        assert_eq!(at("#chapter-one"), heading_in(&book, "one.md", 0));
        assert_eq!(at("#chapter-one-2"), heading_in(&book, "one.md", 1));
        assert_eq!(at("#Chapter One"), heading_in(&book, "one.md", 0));
    }

    /// A book read from one file and cut at its headings is one source,
    /// so its slugs count over the whole file.
    #[test]
    fn slugs_count_over_a_file_cut_into_sections() {
        let text = "# One\n\n## Notes\n\n# Two\n\n## Notes\n";
        let (sections, _) = to_sections(text, "book.md", &Options::default());
        let book = assemble(Metadata::default(), sections);
        assert_eq!(book.sections.len(), 2);
        let anchors = book.anchors();
        assert_eq!(
            anchors.resolve("#notes-2", Some("book.md")),
            heading_in(&book, "book.md", 3),
        );
    }

    /// Acceptance: a heading that writes `{#hunt}` takes `hunt`, and a
    /// later `# Hunt` in the same source takes `hunt-2`.
    #[test]
    fn a_written_id_comes_before_a_slug() {
        let (book, _) = composed(&[("one.md", "# The Chase {#hunt}\n\n# Hunt\n")]);
        let anchors = book.anchors();
        assert_eq!(
            anchors.resolve("#hunt", Some("one.md")),
            heading_in(&book, "one.md", 0)
        );
        assert_eq!(
            anchors.resolve("#hunt-2", Some("one.md")),
            heading_in(&book, "one.md", 1)
        );
    }

    /// Acceptance: an anchor is not written into the tree. The tree the
    /// frontend read serializes the same after assembly, no heading
    /// takes an id, and a byte of the heading still answers with the
    /// run written there.
    #[test]
    fn an_anchor_changes_nothing_in_the_tree() {
        let markdown = "# The Hunter\n\nHe *waited*.\n";
        let (read, _) = to_sections(markdown, "one.md", &Options::default());
        let before = serde_json::to_string(&read).expect("the sections serialize");
        let book = assemble(Metadata::default(), read);
        assert_eq!(
            serde_json::to_string(&book.sections).expect("the sections serialize"),
            before,
        );
        assert!(!before.contains("the-hunter"), "{before}");

        let byte = markdown.find("Hunter").expect("the source holds it") as u32;
        let node = book
            .node_at("one.md", byte)
            .expect("the heading was read there");
        let (_, span) = book.source_of(node).expect("and it says where");
        assert_eq!(
            &markdown[span.start as usize..span.end as usize],
            "The Hunter"
        );
    }

    /// Acceptance: assembled twice, a book resolves each link to the
    /// same node.
    #[test]
    fn anchors_are_deterministic() {
        let sources = [
            ("one.md", "# Chapter One\n\n[[two#Chapter One]]\n"),
            ("two.md", "# Chapter One\n\n# Chapter One\n"),
        ];
        let (first, _) = composed(&sources);
        let (second, _) = composed(&sources);
        assert_eq!(first, second);
        for url in ["two#Chapter One", "two.md#chapter-one-2", "#chapter-one"] {
            assert_eq!(
                first.anchors().resolve(url, Some("one.md")),
                second.anchors().resolve(url, Some("one.md")),
            );
        }
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
            Metadata {
                title: Some("The Levant Papers".into()),
                author: Some("E. Marsh".into()),
                ..Metadata::default()
            },
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
