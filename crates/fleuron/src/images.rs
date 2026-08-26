//! Image sizing: what a header says, and nothing more.
//!
//! Layout needs one thing from an image — how big it is — and getting
//! it by decoding would put a pixel buffer in the layout pass. So the
//! engine reads the header: PNG's `IHDR` and `pHYs`, JPEG's `SOFn` and
//! JFIF density, GIF's screen descriptor, WebP's chunk headers. The
//! pixels stay on disk until a painter asks for them.
//!
//! The engine opens nothing itself, as with fonts: a url means
//! whatever the host says it means.

use serde::Serialize;

use crate::Warning;
use crate::content::{Block, Book};

/// Resolves image urls to bytes.
///
/// Only the head of the file is read, so a host that can stream is
/// free to hand back a prefix.
pub trait ImageLoader {
    /// The bytes behind one url, or `None` when the host cannot
    /// resolve it.
    fn load(&self, url: &str) -> Option<Vec<u8>>;
}

/// A loader that resolves nothing.
pub struct NoImages;

impl ImageLoader for NoImages {
    fn load(&self, _url: &str) -> Option<Vec<u8>> {
        None
    }
}

/// What CSS calls one pixel: 1/96th of an inch. An image whose header
/// declares no resolution is measured at this one.
pub const CSS_DPI: f32 = 96.0;

/// An image's own idea of its size: pixels, and the resolution they
/// are meant to be shown at.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Intrinsic {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Horizontal resolution in pixels per inch.
    pub dpi_x: f32,
    /// Vertical resolution in pixels per inch.
    pub dpi_y: f32,
}

impl Intrinsic {
    /// The intrinsic size in points, at the header's own resolution.
    pub fn size(self) -> (f32, f32) {
        (
            self.width as f32 / self.dpi_x * 72.0,
            self.height as f32 / self.dpi_y * 72.0,
        )
    }
}

/// One image the book refers to, sized from its header.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Asset {
    /// The url the content tree named it by.
    pub url: String,
    /// What the header says it is.
    pub intrinsic: Intrinsic,
}

/// Every image one book refers to, probed once and indexed.
///
/// `DrawItem::Image.asset` indexes this: layout places an image by
/// number, and painters resolve the number back to bytes.
#[derive(Debug, Default)]
pub struct Assets {
    assets: Vec<Asset>,
    warnings: Vec<Warning>,
}

impl Assets {
    /// No images at all: what a host with no loader supplies.
    pub fn none() -> Assets {
        Assets::default()
    }

    /// Probes every image in `book`, in document order. A url the
    /// loader cannot resolve, or bytes no probe recognises, is a
    /// diagnostic and no asset.
    pub fn probe(book: &Book, loader: &dyn ImageLoader) -> Assets {
        let mut assets = Assets::default();
        for section in &book.sections {
            assets.walk(&section.blocks, loader);
        }
        assets
    }

    /// The asset registered for a url, and its index.
    pub fn lookup(&self, url: &str) -> Option<(u32, Intrinsic)> {
        self.assets
            .iter()
            .position(|asset| asset.url == url)
            .map(|index| (index as u32, self.assets[index].intrinsic))
    }

    /// Every asset, in the order `DrawItem::Image.asset` indexes them.
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    /// What probing had to complain about.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    fn walk(&mut self, blocks: &[Block], loader: &dyn ImageLoader) {
        for block in blocks {
            match block {
                Block::Image { url, position, .. } => {
                    if self.lookup(url).is_some() {
                        continue;
                    }
                    let origin = Some(crate::content::origin(None, *position));
                    match loader.load(url).as_deref().and_then(probe) {
                        Some(intrinsic) => self.assets.push(Asset {
                            url: url.clone(),
                            intrinsic,
                        }),
                        None => self.warnings.push(Warning {
                            message: format!("image {url}: no size could be read; it is skipped"),
                            origin,
                        }),
                    }
                }
                Block::Blockquote { blocks, .. } => self.walk(blocks, loader),
                _ => {}
            }
        }
    }
}

/// Reads an image's size from its header. `None` for a format no
/// probe knows, or a header too short to hold one.
pub fn probe(bytes: &[u8]) -> Option<Intrinsic> {
    png(bytes)
        .or_else(|| jpeg(bytes))
        .or_else(|| gif(bytes))
        .or_else(|| webp(bytes))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(slice))
}

fn be16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(slice))
}

fn le16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(slice))
}

/// PNG: `IHDR` is always the first chunk, and `pHYs` — when it is
/// there — says how many pixels go to a metre.
fn png(bytes: &[u8]) -> Option<Intrinsic> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let mut intrinsic = Intrinsic {
        width: be32(bytes, 16)?,
        height: be32(bytes, 20)?,
        dpi_x: CSS_DPI,
        dpi_y: CSS_DPI,
    };
    let mut at = 8usize;
    while let (Some(length), Some(kind)) = (be32(bytes, at), bytes.get(at + 4..at + 8)) {
        if kind == b"pHYs" {
            let data = at + 8;
            // Unit 1 is the metre; anything else is an aspect ratio
            // with no absolute size in it.
            if bytes.get(data + 8) == Some(&1) {
                let (x, y) = (be32(bytes, data)?, be32(bytes, data + 4)?);
                if x > 0 && y > 0 {
                    intrinsic.dpi_x = x as f32 * 0.0254;
                    intrinsic.dpi_y = y as f32 * 0.0254;
                }
            }
            break;
        }
        if kind == b"IDAT" {
            break;
        }
        at = at.checked_add(12)?.checked_add(length as usize)?;
    }
    Some(intrinsic)
}

/// JPEG: walk the marker segments to the frame header, picking up the
/// JFIF density on the way.
fn jpeg(bytes: &[u8]) -> Option<Intrinsic> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let (mut dpi_x, mut dpi_y) = (CSS_DPI, CSS_DPI);
    let mut at = 2usize;
    loop {
        if bytes.get(at) != Some(&0xFF) {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        // Padding fill bytes, and the standalone markers that carry no
        // segment at all.
        if marker == 0xFF {
            at += 1;
            continue;
        }
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            at += 2;
            continue;
        }
        let length = be16(bytes, at + 2)? as usize;
        let data = at + 4;
        if marker == 0xE0 && bytes.get(data..data + 5) == Some(b"JFIF\0") {
            let (x, y) = (be16(bytes, data + 8)?, be16(bytes, data + 10)?);
            let per_inch = match bytes.get(data + 7) {
                Some(1) => Some(1.0),
                Some(2) => Some(2.54),
                _ => None,
            };
            if let Some(scale) = per_inch
                && x > 0
                && y > 0
            {
                dpi_x = x as f32 * scale;
                dpi_y = y as f32 * scale;
            }
        }
        // SOF0-SOF15, less the three markers that share their range
        // and are not frame headers.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some(Intrinsic {
                width: be16(bytes, data + 3)? as u32,
                height: be16(bytes, data + 1)? as u32,
                dpi_x,
                dpi_y,
            });
        }
        if marker == 0xDA {
            return None;
        }
        at = at.checked_add(2)?.checked_add(length)?;
    }
}

/// GIF: the logical screen descriptor, which carries no resolution.
fn gif(bytes: &[u8]) -> Option<Intrinsic> {
    if !bytes.starts_with(b"GIF87a") && !bytes.starts_with(b"GIF89a") {
        return None;
    }
    Some(Intrinsic {
        width: le16(bytes, 6)? as u32,
        height: le16(bytes, 8)? as u32,
        dpi_x: CSS_DPI,
        dpi_y: CSS_DPI,
    })
}

/// WebP: the extended header when there is one, the lossy or lossless
/// frame header otherwise. None of the three carries a resolution.
fn webp(bytes: &[u8]) -> Option<Intrinsic> {
    if !bytes.starts_with(b"RIFF") || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let size = |width: u32, height: u32| {
        Some(Intrinsic {
            width,
            height,
            dpi_x: CSS_DPI,
            dpi_y: CSS_DPI,
        })
    };
    match bytes.get(12..16)? {
        b"VP8X" => {
            let at = 24;
            let three = |from: usize| -> Option<u32> {
                let bytes = bytes.get(from..from + 3)?;
                Some(u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16)
            };
            size(three(at)? + 1, three(at + 3)? + 1)
        }
        b"VP8 " => {
            // The keyframe's start code, then the dimensions with two
            // scale bits above each of them.
            if bytes.get(23..26)? != [0x9D, 0x01, 0x2A] {
                return None;
            }
            size(
                (le16(bytes, 26)? & 0x3FFF) as u32,
                (le16(bytes, 28)? & 0x3FFF) as u32,
            )
        }
        b"VP8L" => {
            if bytes.get(20) != Some(&0x2F) {
                return None;
            }
            let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            size((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header: signature, `IHDR`, and optionally a `pHYs`
    /// declaring pixels per metre.
    fn png_bytes(width: u32, height: u32, ppm: Option<u32>) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(13u32.to_be_bytes());
        bytes.extend(b"IHDR");
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes.extend([0, 0, 0, 0]); // crc
        if let Some(ppm) = ppm {
            bytes.extend(9u32.to_be_bytes());
            bytes.extend(b"pHYs");
            bytes.extend(ppm.to_be_bytes());
            bytes.extend(ppm.to_be_bytes());
            bytes.push(1);
            bytes.extend([0, 0, 0, 0]);
        }
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(b"IDAT");
        bytes
    }

    /// A JPEG header: an optional JFIF density, then a baseline frame.
    fn jpeg_bytes(width: u16, height: u16, density: Option<u16>) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        if let Some(density) = density {
            bytes.extend([0xFF, 0xE0, 0x00, 0x10]);
            bytes.extend(b"JFIF\0");
            bytes.extend([1, 2, 1]); // version, units: inches
            bytes.extend(density.to_be_bytes());
            bytes.extend(density.to_be_bytes());
            bytes.extend([0, 0]);
        }
        bytes.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes
    }

    /// Every format the probe knows, read back at its declared size.
    #[test]
    fn headers_give_up_their_dimensions() {
        assert_eq!(
            probe(&png_bytes(640, 480, None)).map(|i| (i.width, i.height)),
            Some((640, 480)),
        );
        assert_eq!(
            probe(&jpeg_bytes(1200, 900, None)).map(|i| (i.width, i.height)),
            Some((1200, 900)),
        );
        let mut gif = b"GIF89a".to_vec();
        gif.extend(320u16.to_le_bytes());
        gif.extend(200u16.to_le_bytes());
        assert_eq!(probe(&gif).map(|i| (i.width, i.height)), Some((320, 200)));

        let mut webp = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        webp.extend([0, 0, 0, 0, 0, 0, 0, 0]); // chunk size and flags
        webp.extend([0x3F, 0x00, 0x00, 0x1F, 0x00, 0x00]); // 64 x 32, less one
        assert_eq!(probe(&webp).map(|i| (i.width, i.height)), Some((64, 32)));
    }

    /// A header with no resolution in it is measured at 96dpi, and one
    /// with a resolution is measured at that: the same pixels, a
    /// different number of points.
    #[test]
    fn resolution_decides_the_intrinsic_size() {
        let bare = probe(&png_bytes(192, 96, None)).unwrap();
        assert_eq!(bare.size(), (144.0, 72.0));
        // 11811 pixels per metre is 300dpi.
        let dense = probe(&png_bytes(600, 300, Some(11811))).unwrap();
        assert!((dense.dpi_x - 300.0).abs() < 0.1, "{}", dense.dpi_x);
        assert!((dense.size().0 - 144.0).abs() < 0.1, "{:?}", dense.size());
        let inches = probe(&jpeg_bytes(600, 300, Some(300))).unwrap();
        assert_eq!(inches.dpi_x, 300.0);
        assert!((inches.size().0 - 144.0).abs() < 0.1);
    }

    /// Bytes that are not an image, and truncated headers, are `None`
    /// rather than a panic.
    #[test]
    fn unknown_and_truncated_bytes_probe_to_nothing() {
        assert!(probe(b"").is_none());
        assert!(probe(b"not an image at all").is_none());
        let png = png_bytes(4, 4, None);
        for cut in 0..png.len() {
            let _ = probe(&png[..cut]);
        }
        let jpeg = jpeg_bytes(4, 4, Some(72));
        for cut in 0..jpeg.len() {
            let _ = probe(&jpeg[..cut]);
        }
    }

    /// The book's images are probed once each, in document order,
    /// through the host's loader; one the loader cannot resolve is a
    /// diagnostic and no asset.
    #[test]
    fn assets_index_the_book_in_document_order() {
        struct Two;
        impl ImageLoader for Two {
            fn load(&self, url: &str) -> Option<Vec<u8>> {
                match url {
                    "a.png" => Some(png_bytes(96, 48, None)),
                    "b.jpg" => Some(jpeg_bytes(200, 100, None)),
                    _ => None,
                }
            }
        }

        let image = |url: &str| Block::Image {
            id: crate::content::NodeId::UNASSIGNED,
            url: url.into(),
            alt: String::new(),
            position: None,
        };
        let mut book = Book {
            metadata: Default::default(),
            sections: vec![crate::content::Section {
                blocks: vec![
                    image("a.png"),
                    Block::Blockquote {
                        id: crate::content::NodeId::UNASSIGNED,
                        blocks: vec![image("b.jpg")],
                        position: None,
                    },
                    image("a.png"),
                    image("missing.png"),
                ],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        let assets = Assets::probe(&book, &Two);
        assert_eq!(assets.assets().len(), 2, "a.png was probed twice");
        assert_eq!(assets.lookup("a.png").map(|(index, _)| index), Some(0));
        assert_eq!(assets.lookup("b.jpg").map(|(index, _)| index), Some(1));
        assert_eq!(assets.lookup("missing.png"), None);
        assert_eq!(assets.warnings().len(), 1);
        assert!(assets.warnings()[0].message.contains("missing.png"));
    }
}
