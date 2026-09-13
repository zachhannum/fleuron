//! A whole novel with its pages centered down the content box and a
//! height on every chapter heading.
//!
//! Every chapter ends short of the foot of its last page, so every
//! chapter moves at least one page. A book-scale run is where a layout
//! that depends on anything but its input shows.

use fleuron::layout::layout_book;
use fleuron::style::{Source, Stylesheets};
use fleuron_fixtures::{Corpus, registry};

/// The sheet the novel is laid out under.
const CSS: &str = "@page { align-content: center } h2 { min-height: 2in }";

/// Acceptance: two runs over a book with centered pages and headings
/// with a height are byte-identical, and the page count does not move.
#[test]
fn a_novel_of_centered_pages_lays_out_the_same_way_twice() {
    let book = Corpus::GATE.book();
    let styles =
        Stylesheets::parse(&[Source::author("centered.css", CSS)]).compile(&book, registry());
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
        "the headings with a height changed nothing, so nothing is proved",
    );
    assert!(once.warnings.is_empty(), "{:?}", once.warnings);
    assert_eq!(once.pages.len(), twice.pages.len());
    assert_eq!(
        fleuron::wire::encode(&once).expect("a display structure encodes"),
        fleuron::wire::encode(&twice).expect("a display structure encodes"),
    );
}
