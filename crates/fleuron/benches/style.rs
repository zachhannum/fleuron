//! Style compilation: a content tree and CSS in, a style tree out.
//!
//! Timed over the whole book because that is the call shape: one
//! compilation per run, matching every rule against every element.
//! Throughput is in elements, so the number survives a corpus swap.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron_fixtures::{Corpus, registry, styles};

fn style(c: &mut Criterion) {
    let mut group = c.benchmark_group("style");
    for corpus in Corpus::ALL {
        let book = corpus.book();
        group.throughput(Throughput::Elements(styles(&book).nodes().len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &book,
            |b, book| b.iter(|| black_box(fleuron::style::defaults(book, registry()))),
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = style
}
criterion_main!(benches);
