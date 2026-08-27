//! The corpus: public-domain books, checked in as markdown, handed to
//! the harness as content trees. Both texts are Project Gutenberg
//! editions, vendored from the classic-books-markdown collection.
//!
//! Two books, sized for the two questions the harness asks. Pride and
//! Prejudice is the book-scale gate: a complete novel that sets almost
//! exactly the 300 pages the budgets are written against. The Count of
//! Monte Cristo is four times that, and answers whether the pipeline
//! stays linear when a manuscript stops being polite.
//!
//! Real prose, not generated: dialogue by the page, em-dashes, long
//! Latinate words that hyphenation has opinions about, and chapter
//! structure that makes the fragmenter insert real blanks. Generated
//! text is uniform, and uniform text hides the tail cases that make a
//! layout engine slow.
//!
//! The markdown is embedded in the binary rather than read at runtime:
//! the harness runs under wasi with no preopened directories, and a
//! measurement that depends on a working directory is a measurement
//! that reports differently depending on where it was started.

use fleuron::content::{Book, HeadingLevel};
use fleuron_markdown::{Options, Sections};

/// One book in the fixture corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corpus {
    /// ~122k words. The book-scale gate: a complete novel at the 300
    /// pages the budgets are written against.
    PrideAndPrejudice,
    /// ~459k words. Four times the gate, to catch the superlinearity
    /// a single book-sized run cannot see.
    MonteCristo,
}

impl Corpus {
    /// Every book in the corpus, smallest first.
    pub const ALL: [Corpus; 2] = [Corpus::PrideAndPrejudice, Corpus::MonteCristo];

    /// The book the budgets are written against.
    pub const GATE: Corpus = Corpus::PrideAndPrejudice;

    /// Short identifier, for bench names and gate output.
    pub fn slug(self) -> &'static str {
        match self {
            Corpus::PrideAndPrejudice => "pride-and-prejudice",
            Corpus::MonteCristo => "monte-cristo",
        }
    }

    /// The file the markdown was vendored as, and the `source` every
    /// section of the parsed book carries.
    pub fn source(self) -> &'static str {
        match self {
            Corpus::PrideAndPrejudice => "pride-and-prejudice.md",
            Corpus::MonteCristo => "the-count-of-monte-cristo.md",
        }
    }

    /// The vendored markdown.
    pub fn markdown(self) -> &'static str {
        match self {
            Corpus::PrideAndPrejudice => {
                include_str!("../../../fixtures/corpus/pride-and-prejudice.md")
            }
            Corpus::MonteCristo => {
                include_str!("../../../fixtures/corpus/the-count-of-monte-cristo.md")
            }
        }
    }

    /// How the frontend reads this corpus: both books are one file
    /// with a heading per chapter, and Monte Cristo opens its volumes
    /// a level shallower than that.
    pub fn options() -> Options {
        Options {
            sections: Sections::AtHeading(HeadingLevel::H2),
            ..Options::default()
        }
    }

    /// The book as a content tree, node ids assigned. It goes through
    /// the frontend that ships, so the measured path is the shipped
    /// path.
    pub fn book(self) -> Book {
        self.parse(self.markdown())
    }

    /// The same, over markdown the caller holds. The bench and the
    /// gate both time this, so what they time is one call rather than
    /// a stage reached through a private method.
    pub fn parse(self, markdown: &str) -> Book {
        let (sections, _) =
            fleuron_markdown::to_sections(markdown, self.source(), &Self::options());
        fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleuron::content::Block;

    /// Both books parse into content trees with the structure the
    /// harness measures against: metadata off the frontmatter, a
    /// section per chapter, and prose in every one of them.
    #[test]
    fn corpus_books_parse_into_content_trees() {
        for corpus in Corpus::ALL {
            let book = corpus.book();
            assert!(
                book.metadata.title.is_some(),
                "{}: no title parsed",
                corpus.slug()
            );
            assert!(
                book.metadata.author.is_some(),
                "{}: no author parsed",
                corpus.slug()
            );
            assert!(
                book.sections.len() > 40,
                "{}: {} sections, expected a chapter apiece",
                corpus.slug(),
                book.sections.len()
            );
            for section in &book.sections {
                assert_eq!(section.source.as_deref(), Some(corpus.source()));
                assert!(
                    section
                        .blocks
                        .iter()
                        .any(|b| matches!(b, Block::Paragraph { .. } | Block::Heading { .. })),
                    "{}: empty section",
                    corpus.slug()
                );
            }
        }
    }

    /// The gate book is the smaller one: budgets are written against
    /// book scale, and the big book exists to catch superlinearity.
    #[test]
    fn the_gate_book_is_the_smaller_one() {
        assert_eq!(Corpus::GATE, Corpus::ALL[0]);
        assert!(Corpus::PrideAndPrejudice.markdown().len() < Corpus::MonteCristo.markdown().len());
    }
}
