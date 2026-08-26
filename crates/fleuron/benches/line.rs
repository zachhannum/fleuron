//! Line layout: a paragraph's inlines in, broken lines out.
//!
//! The dominant stage, and the one every measure change moves.
//! Throughput is in lines, so the number survives a corpus swap.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::content::{Block, Inline};
use fleuron::lines::{LineBreakOptions, LineLayout};
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

fn line(c: &mut Criterion) {
    let registry = registry();
    let layout = LineLayout::new(registry);
    let mut group = c.benchmark_group("line");
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
            .map(|p| {
                layout
                    .layout(p, body, measure, LineBreakOptions::default())
                    .len()
            })
            .sum();
        group.throughput(Throughput::Elements(lines as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &inlines,
            |b, inlines| {
                b.iter(|| {
                    for paragraph in inlines {
                        black_box(layout.layout(
                            paragraph,
                            body,
                            measure,
                            LineBreakOptions::default(),
                        ));
                    }
                })
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = line
}
criterion_main!(benches);
