//! The warning for a character with no glyph, at the scale of a book.

use fleuron::layout::layout_book;
use fleuron_fixtures::{Corpus, registry, styles};

/// Acceptance: the corpus novel lays out byte-identically to before
/// the change. The digest the perf gate reports holds what it was.
/// Here, the novel has no character the face does not cover, so it
/// reports nothing, and looking for one leaves no mark on the pages:
/// a second run encodes to the same bytes over the same page count.
#[test]
fn the_novel_reports_nothing_and_lays_out_the_same_way_twice() {
    let book = Corpus::GATE.book();
    let styles = styles(&book);
    let assets = fleuron::images::Assets::none();
    let once = layout_book(&book, &styles, registry(), &assets);
    let twice = layout_book(&book, &styles, registry(), &assets);

    assert!(
        once.pages.len() > 100,
        "a novel sets {} pages",
        once.pages.len()
    );
    assert!(once.warnings.is_empty(), "{:?}", once.warnings);
    let encode = |output: &fleuron::LayoutOutput| {
        fleuron::wire::encode(output).expect("a display structure encodes")
    };
    assert_eq!(once.pages.len(), twice.pages.len());
    assert_eq!(encode(&once), encode(&twice));
}
