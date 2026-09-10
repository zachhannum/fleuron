//! The trace stage: a cascade and an asset table in, contours out.
//!
//! Nothing is traced because it is an image. It is traced because a
//! rule that matched it asked for a contour, so the stage is measured
//! over a book whose sheet asks and over the same book whose sheet
//! does not. The second number is the cost a book that wraps to no
//! shape pays, and it is the walk alone.
//!
//! `trace` itself is measured over one image, because that is the
//! call whose cost follows the pixel count. The stage runs it once an
//! asset however many chapters place it.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fleuron::images::{Contours, trace};
use fleuron_fixtures::anchored_images::{
    CONTOUR_CSS, ORNAMENT, ORNAMENT_URL, ORNAMENT_WEBP, assets, illustrated_with,
};
use fleuron_fixtures::{Corpus, registry, styles};

/// One image decoded and its alpha read, in each format that carries
/// one.
fn one_image(c: &mut Criterion) {
    let mut group = c.benchmark_group("trace/image");
    for (format, bytes) in [("png", ORNAMENT), ("webp", ORNAMENT_WEBP)] {
        group.bench_with_input(BenchmarkId::from_parameter(format), &bytes, |b, bytes| {
            b.iter(|| black_box(trace(bytes)))
        });
    }
    group.finish();
}

/// The whole stage over a book with a plate at the head of every
/// chapter, against the same book whose sheet asks for no contour.
fn stage(c: &mut Criterion) {
    let mut group = c.benchmark_group("trace/book");
    for corpus in Corpus::ALL {
        let book = illustrated_with(&corpus.book(), ORNAMENT_URL);
        let assets = assets(&book);
        let wrapping = fleuron::style::Stylesheets::parse(&[fleuron::style::Source::author(
            "trace.css",
            CONTOUR_CSS,
        )])
        .compile(&book, registry());
        let bare = styles(&book);
        group.throughput(Throughput::Elements(book.sections.len() as u64));
        for (asks, styles) in [("a contour", &wrapping), ("no contour", &bare)] {
            group.bench_with_input(
                BenchmarkId::new(corpus.slug(), asks),
                styles,
                |b, styles| {
                    b.iter(|| {
                        let mut contours = Contours::none();
                        black_box(contours.update(&book, styles, &assets));
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
    targets = one_image, stage
}
criterion_main!(benches);
