//! The wire: a display structure as bytes, with a version in front of it.
//!
//! JSON is what the content tree serializes to, because a person
//! reads it. The display structure is machine output: every glyph of
//! every page, produced once per keystroke and decoded on someone's
//! main thread. So it crosses as [postcard], which packs varints,
//! sends no field names, and needs no tree of maps built before the
//! first page can be read.
//!
//! The encoding is positional, which is the price of that: a host's
//! decoder reads fields in declaration order and has no way to notice
//! that the order changed. So a version leads the bytes, [`VERSION`]
//! moves whenever what crosses changes shape, and a host that reads
//! a number it does not know refuses at the first byte instead of
//! painting nonsense.
//!
//! A reply need not carry the whole book: [`encode_range`] sends a
//! slice of the pages and says where it falls, so a host looking at
//! one page pays for one page. Layout still runs over the whole book
//! either way; what a range saves is serializing and decoding the
//! pages nobody asked for. `fonts`, `assets` and `warnings` are never
//! sliced, since none of them is per page.
//!
//! [postcard]: https://postcard.jamesmunns.com/

use crate::fonts::FontRefEntry;
use crate::images::Asset;
use crate::pages::Page;
use crate::{LayoutOutput, Warning};

/// What the encoding is. A host checks this before reading anything
/// else, and a mismatch is a refusal rather than a best effort.
pub const VERSION: u16 = 9;

/// Why a buffer could not be read as a display structure.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The bytes were written by a build that disagrees about the
    /// shape of the display structure.
    #[error("wire version {found}, expected {VERSION}")]
    Version {
        /// The version the buffer leads with.
        found: u16,
    },
    /// The bytes are not a display structure at all, or are truncated.
    #[error("the wire could not be read: {0}")]
    Malformed(#[from] postcard::Error),
}

/// What one wire reply carried, and where it falls in the book.
///
/// Distinct from [`LayoutOutput`], whose `pages` is always the whole
/// run's: a reply's `pages` is a slice, `first` is where that slice
/// begins (counting from 0), and `book_pages` is how many pages the
/// book has, so a reply carrying page 12 alone still answers "page 12
/// of 337".
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Reply {
    /// The pages this reply carries, in reading order.
    pub pages: Vec<Page>,
    /// The index of `pages[0]` in the book. Zero for a whole-book reply.
    pub first: usize,
    /// How many pages the book has, which is more than `pages.len()`
    /// for anything short of a whole-book reply.
    pub book_pages: usize,
    /// The whole run's font table, unsliced.
    pub fonts: Vec<FontRefEntry>,
    /// The whole run's asset table, unsliced.
    pub assets: Vec<Asset>,
    /// The whole run's warnings, unsliced.
    pub warnings: Vec<Warning>,
}

/// Encodes the whole display structure, version first.
pub fn encode(output: &LayoutOutput) -> Result<Vec<u8>, WireError> {
    encode_range(output, 0, output.pages.len())
}

/// Encodes `count` pages starting at `first`, version first. A range
/// past the end of the book is clamped rather than refused: an edit
/// that shortens the book while a page past its new end is still
/// being asked for gets back whatever is left, not a panic.
///
/// `first` and the book's page count travel as `u32`, the same as
/// every other count on the wire; a book past four billion pages is
/// not one this format is sized for.
pub fn encode_range(
    output: &LayoutOutput,
    first: usize,
    count: usize,
) -> Result<Vec<u8>, WireError> {
    let book_pages = output.pages.len();
    let first = first.min(book_pages);
    let end = first.saturating_add(count).min(book_pages);
    let slice = &output.pages[first..end];
    Ok(postcard::to_stdvec(&(
        VERSION,
        first as u32,
        book_pages as u32,
        &output.fonts,
        &output.assets,
        &output.warnings,
        slice,
    ))?)
}

/// Reads a reply back, refusing a version this build does not write.
pub fn decode(bytes: &[u8]) -> Result<Reply, WireError> {
    let (found, rest) = postcard::take_from_bytes::<u16>(bytes)?;
    if found != VERSION {
        return Err(WireError::Version { found });
    }
    let (first, book_pages, fonts, assets, warnings, pages): (
        u32,
        u32,
        Vec<FontRefEntry>,
        Vec<Asset>,
        Vec<Warning>,
        Vec<Page>,
    ) = postcard::from_bytes(rest)?;
    Ok(Reply {
        pages,
        first: first as usize,
        book_pages: book_pages as usize,
        fonts,
        assets,
        warnings,
    })
}

/// The version a buffer leads with, without reading the rest of it.
pub fn version(bytes: &[u8]) -> Result<u16, WireError> {
    Ok(postcard::take_from_bytes::<u16>(bytes)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Warning;
    use crate::content::{NodeId, SourceRange};
    use crate::fonts::{AxisSetting, FaceAttributes, Features, FontRefEntry};
    use crate::images::{Asset, Intrinsic};
    use crate::pages::{DrawItem, Glyph, Page, Side};
    use crate::style::Color;

    fn output() -> LayoutOutput {
        LayoutOutput {
            pages: vec![Page {
                number: 1,
                side: Side::Recto,
                width: 396.0,
                height: 612.0,
                sections: vec![NodeId::UNASSIGNED],
                items: vec![
                    DrawItem::Text {
                        x: 72.0,
                        y: 96.5,
                        font_id: 0,
                        size: 11.0,
                        text: "FI ❦".into(),
                        source: "fi ❦".into(),
                        source_map: vec![0, 1, 2, 3, 4, 5, 6],
                        origin: Some(SourceRange {
                            node: NodeId::UNASSIGNED,
                            range: 3..9,
                        }),
                        features: Features { small_caps: true },
                        color: Color::rgb(180, 30, 30),
                        glyphs: vec![Glyph {
                            id: 42,
                            x: 72.0,
                            range: 0..2,
                        }],
                    },
                    DrawItem::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 396.0,
                        h: 0.5,
                        color: Color::BLACK,
                    },
                    DrawItem::Image {
                        x: 1.0,
                        y: 2.0,
                        w: 3.0,
                        h: 4.0,
                        asset: 7,
                    },
                ],
            }],
            fonts: vec![FontRefEntry {
                family: "eb garamond".into(),
                name: "EB Garamond Regular".into(),
                style: "Regular".into(),
                attributes: FaceAttributes::REGULAR,
                variations: vec![AxisSetting {
                    tag: *b"wght",
                    value: 400.0,
                }],
            }],
            assets: vec![Asset {
                url: "plate.jpg".into(),
                intrinsic: Intrinsic {
                    width: 480,
                    height: 320,
                    dpi_x: 300.0,
                    dpi_y: 300.0,
                },
            }],
            warnings: vec![Warning {
                message: "a table became prose".into(),
                origin: Some("ch01.md:12:1".into()),
            }],
        }
    }

    /// A reply as a `LayoutOutput`, for re-encoding it and comparing
    /// the bytes: sound whenever the reply is a whole book, which is
    /// all the round-trip tests below ask of it.
    fn as_output(reply: Reply) -> LayoutOutput {
        LayoutOutput {
            pages: reply.pages,
            fonts: reply.fonts,
            assets: reply.assets,
            warnings: reply.warnings,
        }
    }

    /// What went out comes back, and going out again writes the same
    /// bytes.
    #[test]
    fn the_wire_round_trips() {
        let bytes = encode(&output()).unwrap();
        let read = decode(&bytes).unwrap();
        assert_eq!(read.pages, output().pages);
        assert_eq!(read.first, 0, "a whole-book reply starts at page 0");
        assert_eq!(
            read.book_pages,
            output().pages.len(),
            "a whole-book reply's total is its own page count"
        );
        assert_eq!(read.fonts, output().fonts);
        assert_eq!(read.assets, output().assets);
        assert_eq!(read.warnings, output().warnings);
        assert_eq!(encode(&as_output(read)).unwrap(), bytes);
    }

    /// The version leads the bytes, so a host reads it before it
    /// commits to anything.
    #[test]
    fn the_version_leads_the_bytes() {
        let bytes = encode(&output()).unwrap();
        assert_eq!(version(&bytes).unwrap(), VERSION);
        assert_eq!(bytes[0], VERSION as u8);
    }

    /// A version this build does not write is refused rather than
    /// read as best it can be.
    #[test]
    fn an_unknown_version_is_refused() {
        let mut bytes = encode(&output()).unwrap();
        bytes[0] = VERSION as u8 + 1;
        assert!(matches!(decode(&bytes), Err(WireError::Version { .. })));
    }

    /// A ranged reply carries only its own slice, but the tables and
    /// the book's total page count are the whole run's.
    #[test]
    fn a_range_carries_only_its_own_slice_with_the_full_tables() {
        let mut book = output();
        book.pages.push(Page {
            number: 2,
            side: Side::Verso,
            width: 396.0,
            height: 612.0,
            sections: vec![],
            items: vec![],
        });
        let bytes = encode_range(&book, 1, 1).unwrap();
        let read = decode(&bytes).unwrap();
        assert_eq!(read.pages, book.pages[1..2]);
        assert_eq!(read.first, 1);
        assert_eq!(read.book_pages, 2);
        assert_eq!(read.fonts, book.fonts, "the font table is not sliced");
        assert_eq!(read.assets, book.assets, "the asset table is not sliced");
        assert_eq!(read.warnings, book.warnings, "the warnings are not sliced");
    }

    /// A range past the end of the book is clamped to what is left
    /// rather than refused or panicking: a page asked for from a book
    /// an edit just shortened still gets an answer.
    #[test]
    fn a_range_past_the_end_clamps_rather_than_panics() {
        let book = output();
        let bytes = encode_range(&book, 5, 3).unwrap();
        let read = decode(&bytes).unwrap();
        assert!(read.pages.is_empty());
        assert_eq!(read.first, 1, "first clamps to the book's own length");
        assert_eq!(read.book_pages, 1);
    }
}
