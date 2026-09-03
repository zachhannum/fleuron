//! Pagination: a content tree in, a numbered structure out.
//!
//! The composed stage — line layout and fragmentation together, as a
//! caller pays for them. Its own number matters less than the gap
//! between it and the sum of the two stages beneath it.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::layout::Paginator;
use fleuron_fixtures::{Corpus, registry, styles};

fn paginate(c: &mut Criterion) {
    let mut group = c.benchmark_group("paginate");
    for corpus in Corpus::ALL {
        let book = corpus.book();
        let styles = styles(&book);
        let paginator = Paginator::new(registry(), &styles);
        group.throughput(Throughput::Elements(paginator.paginate(&book).len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &book,
            |b, book| b.iter(|| black_box(paginator.paginate(book))),
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = paginate
}
criterion_main!(benches);
