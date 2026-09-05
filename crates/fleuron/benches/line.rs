//! Line layout: a paragraph's inlines in, broken lines out.
//!
//! The dominant stage, and the one every measure change moves.
//! Throughput is in lines, so the number survives a corpus swap.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::content::{Block, Inline};
use fleuron::lines::{HangingPunctuation, LineBreakOptions, LineLayout, Patterns};
use fleuron_fixtures::{Corpus, registry, styles};

/// Every paragraph in the book, in document order. Headings are left
/// out: there are two orders of magnitude fewer of them, and they
/// break against a different measure-to-size ratio.
fn paragraphs<'a>(blocks: &'a [Block], out: &mut Vec<&'a [Inline]>) {
    for block in blocks {
        match block {
            Block::Paragraph { inlines, .. } => out.push(inlines),
            Block::Blockquote { blocks, .. } => paragraphs(blocks, out),
            _ => {}
        }
    }
}

/// The two settings a book is set in. Justification costs the glue
/// adjustment on every line and the wider search that comes with
/// having glue to adjust, so it is timed apart from ragged.
const SETTINGS: [(&str, LineBreakOptions); 2] = [
    (
        "ragged",
        LineBreakOptions {
            hyphenate: false,
            patterns: Patterns::ENGLISH,
            justify: false,
            inter_character: false,
            hanging: HangingPunctuation::NONE,
        },
    ),
    (
        "justified",
        LineBreakOptions {
            hyphenate: true,
            patterns: Patterns::ENGLISH,
            justify: true,
            inter_character: false,
            hanging: HangingPunctuation::NONE,
        },
    ),
];

fn line(c: &mut Criterion) {
    let registry = registry();
    let layout = LineLayout::new(registry);
    let mut group = c.benchmark_group("line");
    for (setting, options) in SETTINGS {
        for corpus in Corpus::ALL {
            let book = corpus.book();
            let styles = styles(&book);
            let measure = styles.default_page().geometry.measure();
            let body = styles.root().paragraph();
            let mut inlines = Vec::new();
            for section in &book.sections {
                paragraphs(&section.blocks, &mut inlines);
            }
            let lines: usize = inlines
                .iter()
                .map(|p| layout.layout(p, body, measure, options).len())
                .sum();
            group.throughput(Throughput::Elements(lines as u64));
            group.bench_with_input(
                BenchmarkId::new(setting, corpus.slug()),
                &inlines,
                |b, inlines| {
                    b.iter(|| {
                        for paragraph in inlines {
                            black_box(layout.layout(paragraph, body, measure, options));
                        }
                    })
                },
            );
        }
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = line
}
criterion_main!(benches);
