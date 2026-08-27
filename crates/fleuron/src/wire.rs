//! The wire: a display list as bytes, with a version in front of it.
//!
//! JSON is what the content tree serializes to, because a person
//! reads it. The display list is machine output: every glyph of
//! every page, produced once per keystroke and decoded on someone's
//! main thread. So it crosses as [postcard], which packs varints,
//! sends no field names, and needs no tree of maps built before the
//! first page can be read.
//!
//! The encoding is positional, which is the price of that: a host's
//! decoder reads fields in declaration order and has no way to notice
//! that the order changed. So a version leads the bytes, [`VERSION`]
//! moves whenever the display list's shape does, and a host that
//! reads a number it does not know refuses at the first byte instead
//! of painting nonsense.
//!
//! [postcard]: https://postcard.jamesmunns.com/

use crate::LayoutOutput;

/// What the encoding is. A host checks this before reading anything
/// else, and a mismatch is a refusal rather than a best effort.
pub const VERSION: u16 = 3;

/// Why a buffer could not be read as a display list.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The bytes were written by a build that disagrees about the
    /// shape of the display list.
    #[error("wire version {found}, expected {VERSION}")]
    Version {
        /// The version the buffer leads with.
        found: u16,
    },
    /// The bytes are not a display list at all, or are truncated.
    #[error("the wire could not be read: {0}")]
    Malformed(#[from] postcard::Error),
}

/// Encodes a display list, version first.
pub fn encode(output: &LayoutOutput) -> Result<Vec<u8>, WireError> {
    Ok(postcard::to_stdvec(&(VERSION, output))?)
}

/// Reads a display list back, refusing a version this build does not
/// write.
pub fn decode(bytes: &[u8]) -> Result<LayoutOutput, WireError> {
    let (found, rest) = postcard::take_from_bytes::<u16>(bytes)?;
    if found != VERSION {
        return Err(WireError::Version { found });
    }
    Ok(postcard::from_bytes(rest)?)
}

/// The version a buffer leads with, without reading the rest of it.
pub fn version(bytes: &[u8]) -> Result<u16, WireError> {
    Ok(postcard::take_from_bytes::<u16>(bytes)?.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Warning;
    use crate::fonts::{AxisSetting, FaceAttributes, FontRefEntry};
    use crate::images::{Asset, Intrinsic};
    use crate::pages::{DrawItem, Glyph, Page, Side};

    fn output() -> LayoutOutput {
        LayoutOutput {
            pages: vec![Page {
                number: 1,
                side: Side::Recto,
                width: 396.0,
                height: 612.0,
                items: vec![
                    DrawItem::Text {
                        x: 72.0,
                        y: 96.5,
                        font_id: 0,
                        size: 11.0,
                        text: "fi ❦".into(),
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

    /// What went out comes back, and going out again writes the same
    /// bytes.
    #[test]
    fn the_wire_round_trips() {
        let bytes = encode(&output()).unwrap();
        let read = decode(&bytes).unwrap();
        assert_eq!(read, output());
        assert_eq!(encode(&read).unwrap(), bytes);
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
}
