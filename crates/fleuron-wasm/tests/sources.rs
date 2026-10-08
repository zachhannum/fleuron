//! A source keeps the place the book gave it, whatever its text is
//! edited to.

use fleuron::pages::DrawItem;
use fleuron::wire;
use fleuron_wasm::Session;

const NAMES: [&str; 3] = ["a.md", "b.md", "c.md"];
const TEXTS: [&str; 3] = ["Alpha.\n", "Bravo.\n", "Charlie.\n"];

/// A book of `texts`, one per name.
fn book(texts: [&str; 3]) -> Session {
    let mut session = Session::new().unwrap();
    session
        .set_sources(
            NAMES.map(String::from).to_vec(),
            texts.map(String::from).to_vec(),
            None,
        )
        .unwrap();
    session
}

/// The paragraphs of the book in the order the pages set them.
fn order(session: &mut Session) -> Vec<String> {
    let reply = wire::decode(&session.preview(None, None).unwrap()).expect("the reply reads back");
    reply
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { text, .. } => Some(text.trim().to_string()),
            _ => None,
        })
        .filter(|text| TEXTS.iter().any(|known| known.trim() == text))
        .collect()
}

/// Acceptance: a source edited to a text with no blocks, and then
/// edited back, is set at the place the book op gave it.
#[test]
fn a_source_edited_to_no_blocks_and_back_keeps_its_place() {
    for blank in ["", " \n\t\n", "---\ntitle: Alpha\n---\n"] {
        let mut session = book(TEXTS);
        session.update_markdown("a.md", blank);
        assert_eq!(order(&mut session), ["Bravo.", "Charlie."], "{blank:?}");

        session.update_markdown("a.md", TEXTS[0]);
        assert_eq!(
            order(&mut session),
            ["Alpha.", "Bravo.", "Charlie."],
            "{blank:?}"
        );
    }
}

/// Acceptance: a source that is empty when the book opens, and is
/// then given text, is set at the place the book op gave it.
#[test]
fn a_source_empty_when_the_book_opens_is_set_at_its_place() {
    for empty in 0..NAMES.len() {
        let mut texts = TEXTS;
        texts[empty] = "";
        let mut session = book(texts);
        session.update_markdown(NAMES[empty], TEXTS[empty]);
        assert_eq!(
            order(&mut session),
            ["Alpha.", "Bravo.", "Charlie."],
            "{}",
            NAMES[empty]
        );
    }
}

/// A source the host removed has no place left, so the same name
/// arrives as a new file does.
#[test]
fn a_removed_source_comes_back_at_the_end() {
    let mut session = book(TEXTS);
    session.remove_markdown("a.md");
    session.update_markdown("a.md", TEXTS[0]);
    assert_eq!(order(&mut session), ["Bravo.", "Charlie.", "Alpha."]);
}
