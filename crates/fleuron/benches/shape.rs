//! Shaping: text in, positioned glyphs out.
//!
//! Timed per block, over the whole book, because that is the call
//! shape line layout makes — one shaping pass per style run, and v0.1
//! puts one face on everything.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::lines::ParagraphStyle;
use fleuron_fixtures::{Corpus, registry, shaped_texts};

fn shape(c: &mut Criterion) {
    let registry = registry();
    let mut group = c.benchmark_group("shape");
    for corpus in Corpus::ALL {
        let texts = shaped_texts(&corpus.book());
        let bytes: usize = texts.iter().map(String::len).sum();
        group.throughput(Throughput::Bytes(bytes as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(corpus.slug()),
            &texts,
            |b, texts| {
                b.iter(|| {
                    for text in texts {
                        black_box(registry.shape(ParagraphStyle::BODY.font_id, text));
                    }
                })
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    // A book is one iteration; ten of them says enough, and the
    // default hundred says the same thing over several minutes.
    config = Criterion::default().sample_size(10);
    targets = shape
}
criterion_main!(benches);
