//! The corpus through the public frontend.
//!
//! The perf budgets are written against these two books, so the tree
//! the harness measures has to be the tree the frontend produces. The
//! digest below covers everything layout reads: metadata, section
//! sources, block and inline structure, and every character of prose.
//! Source positions are left out, being diagnostic data, and they moved
//! when the corpus put its metadata in frontmatter.

use std::hash::{DefaultHasher, Hash, Hasher};

use fleuron::content::{Block, Book, Inline, Metadata};
use fleuron_fixtures::Corpus;
use fleuron_markdown::{Options, Sections, assemble, to_sections};

/// What each corpus book hashes to. A digest that moves is a change
/// to the mapping, and page counts move with it.
const DIGESTS: [(Corpus, u64); 2] = [
    (Corpus::PrideAndPrejudice, 9189800476242993549),
    (Corpus::MonteCristo, 2340015116922178090),
];

#[test]
fn the_corpus_reads_the_same_through_the_frontend() {
    for (corpus, expected) in DIGESTS {
        let book = corpus.book();
        let found = digest(&book);
        assert_eq!(found, expected, "{}", corpus.slug());
    }
}

fn digest(book: &Book) -> u64 {
    let mut hasher = DefaultHasher::new();
    book.metadata.title.hash(&mut hasher);
    book.metadata.author.hash(&mut hasher);
    book.metadata.extra.hash(&mut hasher);
    book.sections.len().hash(&mut hasher);
    for section in &book.sections {
        section.source.hash(&mut hasher);
        section.title.hash(&mut hasher);
        hash_blocks(&section.blocks, &mut hasher);
    }
    hasher.finish()
}

fn hash_blocks(blocks: &[Block], hasher: &mut DefaultHasher) {
    blocks.len().hash(hasher);
    for block in blocks {
        match block {
            Block::Heading { level, inlines, .. } => {
                ("heading", u8::from(*level)).hash(hasher);
                hash_inlines(inlines, hasher);
            }
            Block::Paragraph { inlines, .. } => {
                "paragraph".hash(hasher);
                hash_inlines(inlines, hasher);
            }
            Block::Blockquote { blocks, .. } => {
                "blockquote".hash(hasher);
                hash_blocks(blocks, hasher);
            }
            Block::ThematicBreak { .. } => "thematic_break".hash(hasher),
            Block::Image { url, alt, .. } => ("image", url, alt).hash(hasher),
        }
    }
}

fn hash_inlines(inlines: &[Inline], hasher: &mut DefaultHasher) {
    inlines.len().hash(hasher);
    for inline in inlines {
        match inline {
            Inline::Text { value, .. } => ("text", value).hash(hasher),
            Inline::Code { value, .. } => ("code", value).hash(hasher),
            Inline::Emphasis { children, .. } => {
                "emphasis".hash(hasher);
                hash_inlines(children, hasher);
            }
            Inline::Strong { children, .. } => {
                "strong".hash(hasher);
                hash_inlines(children, hasher);
            }
            Inline::Link { url, children, .. } => {
                ("link", url).hash(hasher);
                hash_inlines(children, hasher);
            }
        }
    }
}

/// The two shapes a manuscript arrives in are the same manuscript.
///
/// Pride and Prejudice is one file of sixty-one chapters; a vault is
/// sixty-one files of one. Split it and read each piece as a whole
/// source, and the tree is the tree the single file produced, section
/// for section and block for block. What differs is `source`, which
/// now names a chapter file, and the positions inside it, which count
/// from the top of that file rather than the top of the book.
#[test]
fn one_file_of_chapters_and_a_file_per_chapter_read_alike() {
    let corpus = Corpus::PrideAndPrejudice;
    let (sections, _) = to_sections(corpus.markdown(), corpus.source(), &Corpus::options());
    let whole = assemble(fleuron_markdown::frontmatter(corpus.markdown()), sections);

    let per_file = Options {
        sections: Sections::Whole,
        ..Options::default()
    };
    let mut split = Vec::new();
    for (index, chapter) in chapters(corpus.markdown()).iter().enumerate() {
        let source = format!("chapter-{:02}.md", index + 1);
        split.extend(to_sections(chapter, &source, &per_file).0);
    }
    let composed = assemble(whole.metadata.clone(), split);

    assert_eq!(composed.sections.len(), 61);
    assert_eq!(anonymous(&whole), anonymous(&composed));
}

/// Cuts a manuscript at every chapter heading. The frontmatter leads
/// and holds no chapter, which is why the first piece contributes no
/// section.
fn chapters(markdown: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    for line in markdown.lines() {
        if line.starts_with("## ") && !current.is_empty() {
            pieces.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    pieces.push(current);
    pieces
}

/// The book with everything that names a file stripped out: what is
/// left is what two arrangements of the same manuscript share.
fn anonymous(book: &Book) -> Book {
    let mut book = book.clone();
    book.metadata = Metadata::default();
    for section in &mut book.sections {
        section.source = None;
        section.position = None;
        strip_blocks(&mut section.blocks);
    }
    book
}

fn strip_blocks(blocks: &mut [Block]) {
    for block in blocks {
        match block {
            Block::Heading {
                position, inlines, ..
            }
            | Block::Paragraph {
                position, inlines, ..
            } => {
                *position = None;
                strip_inlines(inlines);
            }
            Block::Blockquote {
                position, blocks, ..
            } => {
                *position = None;
                strip_blocks(blocks);
            }
            Block::ThematicBreak { position, .. } | Block::Image { position, .. } => {
                *position = None
            }
        }
    }
}

fn strip_inlines(inlines: &mut [Inline]) {
    for inline in inlines {
        match inline {
            Inline::Text { position, .. } | Inline::Code { position, .. } => *position = None,
            Inline::Emphasis {
                position, children, ..
            }
            | Inline::Strong {
                position, children, ..
            }
            | Inline::Link {
                position, children, ..
            } => {
                *position = None;
                strip_inlines(children);
            }
        }
    }
}
