//! Fragmentation: laid-out lines in, pages out.
//!
//! Measured over lines that were broken beforehand, so the number is
//! the cost of deciding where pages end and of building the display
//! list — never of measuring text again.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::layout::{PageGeometry, Paginator};
use fleuron::lines::Line;
use fleuron_fixtures::{Corpus, registry};

fn fragment(c: &mut Criterion) {
    let paginator = Paginator::new(registry(), PageGeometry::trade_paperback());
    let mut group = c.benchmark_group("fragment");
    for corpus in Corpus::ALL {
        let book = corpus.book();
        let sections: Vec<Vec<Line>> = book
            .sections
            .iter()
            .map(|section| paginator.section_lines(section))
            .collect();
        group.throughput(Throughput::Elements(paginator.flow(&sections).len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &sections,
            |b, sections| b.iter(|| black_box(paginator.flow(sections))),
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = fragment
}
criterion_main!(benches);
