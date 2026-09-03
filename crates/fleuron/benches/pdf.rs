//! PDF writing: display structure in, bytes out.
//!
//! Laid out once, up front: this stage's cost is font subsetting and
//! content-stream serialization, and none of that is layout.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::images::Assets;
use fleuron::pdf;
use fleuron_fixtures::{Corpus, registry, styles};

fn write(c: &mut Criterion) {
    let registry = registry();
    let mut group = c.benchmark_group("pdf");
    for corpus in Corpus::ALL {
        let book = corpus.book();
        let output = fleuron::layout::layout_book(&book, &styles(&book), registry, &Assets::none());
        group.throughput(Throughput::Elements(output.pages.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &output,
            |b, output| {
                b.iter(|| {
                    black_box(
                        pdf::write(output, registry, &Assets::none(), &book.metadata)
                            .expect("writes"),
                    )
                })
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10);
    targets = write
}
criterion_main!(benches);
