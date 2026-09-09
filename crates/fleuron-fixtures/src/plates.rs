//! Plates: the corpus books with images anchored to the page.
//!
//! An illustrated edition puts a plate at the head of a chapter and
//! sets the opening prose around it. That is the path a book without
//! images never reaches: the flow settles where each plate lands,
//! places it, and breaks the paragraphs beside it again. The gate
//! measures the same book both ways so the cost of the path is a
//! number rather than a guess.

use fleuron::content::{Attributes, Block, Book, NodeId};
use fleuron::images::{Assets, ImageLoader};

/// The plate every chapter of a plated book opens with: the map that
/// faces page 1 of the fixture book, 180pt wide.
pub const PLATE: &[u8] = include_bytes!("../../../fixtures/images/plate.jpg");

/// What the plated book calls it.
pub const URL: &str = "plate.jpg";

/// The sheet that anchors it: at the outer corner of the page area,
/// with the chapter's opening prose set beside it.
pub const CSS: &str = "img { position: absolute; top: 0; right: 0; \
                       margin-left: 12pt; margin-bottom: 6pt; wrap-flow: start }";

/// The same book with a plate at the head of every chapter, anchored
/// above the prose the chapter opens with.
pub fn plated(book: &Book) -> Book {
    let mut plated = book.clone();
    for section in &mut plated.sections {
        let at = section
            .blocks
            .iter()
            .position(|block| !matches!(block, Block::Heading { .. }))
            .unwrap_or(section.blocks.len());
        section.blocks.insert(
            at,
            Block::Image {
                id: NodeId::UNASSIGNED,
                url: URL.into(),
                alt: "a map of Lilliput".into(),
                attributes: Attributes::default(),
                position: None,
                span: None,
            },
        );
    }
    plated.assign_node_ids();
    plated
}

/// The plate itself, as a table layout can size from.
pub fn assets(book: &Book) -> Assets {
    struct Plate;
    impl ImageLoader for Plate {
        fn load(&self, url: &str) -> Option<Vec<u8>> {
            (url == URL).then(|| PLATE.to_vec())
        }
    }
    Assets::probe(book, &Plate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Corpus;

    /// Every chapter of a plated book carries one plate, and it sits
    /// above the prose rather than under the heading.
    #[test]
    fn every_chapter_of_a_plated_book_opens_with_one() {
        let book = plated(&Corpus::GATE.book());
        for section in &book.sections {
            let images = section
                .blocks
                .iter()
                .filter(|block| matches!(block, Block::Image { .. }))
                .count();
            assert_eq!(images, 1, "one plate a chapter");
        }
        let assets = assets(&book);
        assert_eq!(assets.assets().len(), 1, "one image, however often placed");
    }
}
