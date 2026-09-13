//! A whole novel with positioned blocks in every chapter.
//!
//! The sheet moves every chapter heading up, and lifts the paragraph
//! that opens each chapter against the page. The prose of that page
//! wraps beside the paragraph. A run at book scale exposes a layout
//! that depends on anything but its input.

use fleuron::layout::layout_book;
use fleuron::style::{Source, Stylesheets};
use fleuron_fixtures::{Corpus, registry};

/// The sheet the novel is laid out under.
const CSS: &str = "h2 { position: relative; top: -12pt } \
                   h2 + p { position: absolute; top: 0; left: 50%; margin-left: 12pt; \
                   wrap-flow: start; background-color: #f4f1ea }";

/// Acceptance: two runs over a book with positioned blocks are
/// byte-identical, and the page count does not move.
#[test]
fn a_novel_of_positioned_blocks_lays_out_the_same_way_twice() {
    let book = Corpus::GATE.book();
    let styles =
        Stylesheets::parse(&[Source::author("positioned.css", CSS)]).compile(&book, registry());
    assert!(styles.warnings().is_empty(), "{:?}", styles.warnings());
    let assets = fleuron::images::Assets::none();
    let once = layout_book(&book, &styles, registry(), &assets);
    let twice = layout_book(&book, &styles, registry(), &assets);

    let bare = layout_book(&book, &fleuron_fixtures::styles(&book), registry(), &assets);
    assert!(
        once.pages.len() > 100,
        "a novel sets {} pages",
        once.pages.len()
    );
    assert_ne!(
        once.pages.len(),
        bare.pages.len(),
        "the lifted paragraphs changed nothing, so nothing is proved",
    );
    assert!(once.warnings.is_empty(), "{:?}", once.warnings);
    assert_eq!(once.pages.len(), twice.pages.len());
    assert_eq!(
        fleuron::wire::encode(&once).expect("a display structure encodes"),
        fleuron::wire::encode(&twice).expect("a display structure encodes"),
    );
}
