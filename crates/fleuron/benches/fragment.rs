//! Fragmentation: laid-out lines in, pages out.
//!
//! Measured over lines that were broken beforehand, so the number is
//! the cost of deciding where pages end and of building the display
//! list — never of measuring text again.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::layout::{Fragment, Paginator};
use fleuron_fixtures::{Corpus, registry, styles};

fn fragment(c: &mut Criterion) {
    let mut group = c.benchmark_group("fragment");
    for corpus in Corpus::ALL {
        let book = corpus.book();
        let styles = styles(&book);
        let paginator = Paginator::new(registry(), &styles);
        let sections: Vec<Vec<Fragment>> = book
            .sections
            .iter()
            .map(|section| paginator.section_fragments(section))
            .collect();
        group.throughput(Throughput::Elements(
            paginator.flow(&book, &sections).len() as u64
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &sections,
            |b, sections| b.iter(|| black_box(paginator.flow(&book, sections))),
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
