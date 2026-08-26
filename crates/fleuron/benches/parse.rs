//! The frontend: markdown in, a content tree out.
//!
//! Timed over the whole source because that is the call shape: a
//! manuscript arrives as a file and becomes sections in one pass.
//! Throughput is in bytes, which is what a parse's cost tracks and
//! what survives a corpus swap.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron_fixtures::Corpus;

fn parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for corpus in Corpus::ALL {
        let markdown = corpus.markdown();
        group.throughput(Throughput::Bytes(markdown.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &markdown,
            |b, markdown| b.iter(|| black_box(corpus.parse(markdown))),
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = parse
}
criterion_main!(benches);
