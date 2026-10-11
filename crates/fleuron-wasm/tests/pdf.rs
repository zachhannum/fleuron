//! The export pads a book to the page count a print vendor asks for.

use fleuron::wire;
use fleuron_wasm::Session;

fn session() -> Session {
    let mut session = Session::new().unwrap();
    session.set_markdown("book.md", "# One\n\nAlpha.\n");
    session
}

/// How many pages the file's page tree counts.
fn counted(pdf: &[u8]) -> u32 {
    let text = String::from_utf8_lossy(pdf);
    let (_, rest) = text
        .split_once("/Type /Pages\n  /Count ")
        .expect("a page tree");
    let (count, _) = rest.split_once('\n').expect("a count");
    count.parse().expect("a number")
}

/// Acceptance: `exportPdf` takes the count to pad to, an even one or
/// a multiple of four, and the session says how many pages the file
/// holds.
#[test]
fn the_export_pads_to_the_count_it_is_asked_for() {
    let mut session = session();
    let laid_out = wire::decode(&session.preview(None, None).unwrap())
        .expect("the reply reads back")
        .pages
        .len() as u32;
    assert_eq!(laid_out, 1, "the book is one page");

    for (multiple_of, pages) in [(None, 1), (Some(2), 2), (Some(4), 4)] {
        assert_eq!(session.pdf_pages(multiple_of).unwrap(), pages);
        assert_eq!(counted(&session.export_pdf(multiple_of).unwrap()), pages);
    }
}

/// Acceptance: the padding runs no stage, and the preview answers
/// with the bytes it answered with before.
#[test]
fn padding_leaves_the_preview_as_it_was() {
    let mut session = session();
    let before = session.preview(None, None).unwrap();
    let settled = session.stages();

    session.export_pdf(Some(4)).unwrap();
    assert_eq!(session.stages(), settled, "padding re-ran a stage");
    assert_eq!(session.preview(None, None).unwrap(), before);
}
