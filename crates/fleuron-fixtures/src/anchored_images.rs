//! The corpus books with images anchored to the page.
//!
//! A book with images anchored to its pages reaches a path a book
//! without them never does. The flow settles where each image lands,
//! places it, and breaks the paragraphs beside it again. The gate
//! measures the same book both ways, so the cost of that path is a
//! number rather than a guess.

use fleuron::content::{Attributes, Block, Book, NodeId};
use fleuron::images::{Assets, ImageLoader};

/// The image every chapter of an illustrated book opens with: the map
/// that faces page 1 of the fixture book, 180pt wide.
pub const IMAGE: &[u8] = include_bytes!("../../../fixtures/images/plate.jpg");

/// What the book calls it.
pub const URL: &str = "plate.jpg";

/// The sheet that anchors it: at the outer corner of the page area,
/// with the chapter's opening prose set beside it.
pub const CSS: &str = "img { position: absolute; top: 0; right: 0; \
                       margin-left: 12pt; margin-bottom: 6pt; wrap-flow: start }";

/// The same book with an image at the head of every chapter, anchored
/// above the prose the chapter opens with.
pub fn illustrated(book: &Book) -> Book {
    let mut illustrated = book.clone();
    for section in &mut illustrated.sections {
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
    illustrated.assign_node_ids();
    illustrated
}

/// The image itself, in an asset table that layout can size from.
pub fn assets(book: &Book) -> Assets {
    struct Image;
    impl ImageLoader for Image {
        fn load(&self, url: &str) -> Option<Vec<u8>> {
            (url == URL).then(|| IMAGE.to_vec())
        }
    }
    Assets::probe(book, &Image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Corpus;

    /// Every chapter of an illustrated book carries one image, and it
    /// sits above the prose rather than under the heading.
    #[test]
    fn every_chapter_of_an_illustrated_book_opens_with_one() {
        let book = illustrated(&Corpus::GATE.book());
        for section in &book.sections {
            let images = section
                .blocks
                .iter()
                .filter(|block| matches!(block, Block::Image { .. }))
                .count();
            assert_eq!(images, 1, "one image a chapter");
        }
        let assets = assets(&book);
        assert_eq!(assets.assets().len(), 1, "one image, however often placed");
    }
}
