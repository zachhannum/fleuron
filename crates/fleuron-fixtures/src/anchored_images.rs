//! The corpus books with images anchored to the page.
//!
//! A book with images anchored to its pages reaches a path a book
//! without them never does. The flow settles where each image lands,
//! places it, and breaks the paragraphs beside it again. The gate
//! measures the same book both ways, so the cost of that path is a
//! number rather than a guess.
//!
//! [`ORNAMENT`] is the second image here, and it is the one the trace
//! bench reads: it carries an alpha channel, which the map does not.

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

/// The ornament the fixture book closes a chapter with, which is the
/// image in this repository that carries an alpha channel.
pub const ORNAMENT: &[u8] = include_bytes!("../../../fixtures/images/fleuron.png");

/// The same ornament as WebP: the same pixels in the other format
/// that carries alpha.
pub const ORNAMENT_WEBP: &[u8] = include_bytes!("../../../fixtures/images/fleuron.webp");

/// What a book that wraps prose to a traced contour names it.
pub const ORNAMENT_URL: &str = "fleuron.png";

/// The sheet that wraps prose to the ornament's own shape.
pub const CONTOUR_CSS: &str = "img { position: absolute; top: 0; right: 0; \
                               margin-left: 12pt; wrap-flow: start; \
                               shape-outside: auto; shape-margin: 6pt }";

/// The same book with an image at the head of every chapter, anchored
/// above the prose the chapter opens with.
pub fn illustrated(book: &Book) -> Book {
    illustrated_with(book, URL)
}

/// The same, over an image the caller names.
pub fn illustrated_with(book: &Book, url: &str) -> Book {
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
                url: url.into(),
                alt: "the plate the chapter opens with".into(),
                attributes: Attributes::default(),
                position: None,
                span: None,
            },
        );
    }
    illustrated.assign_node_ids();
    illustrated
}

/// The images themselves, in an asset table that layout can size
/// from.
pub fn assets(book: &Book) -> Assets {
    struct Images;
    impl ImageLoader for Images {
        fn load(&self, url: &str) -> Option<Vec<u8>> {
            match url {
                URL => Some(IMAGE.to_vec()),
                ORNAMENT_URL => Some(ORNAMENT.to_vec()),
                _ => None,
            }
        }
    }
    Assets::probe(book, &Images)
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
