//! The files a book refers to by url, copied into the container: the
//! images of the book and its sheets, and the fonts of its sheets.
//! Nothing here decodes a file. The first bytes name its type.

use std::collections::BTreeMap;

/// What kind of file a url names, which is what decides the type the
/// bytes have to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Image,
    Font,
}

/// One file in the container.
#[derive(Debug, Clone)]
pub struct Resource {
    /// Its path in the container, from the package directory.
    pub href: String,
    /// Its media type, for the manifest.
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Why a url did not become a file in the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The host had no bytes for it.
    Missing,
    /// The bytes are not a type an EPUB can hold.
    Unknown,
}

/// Every file the book refers to, in the order it was first asked
/// for.
#[derive(Debug, Default)]
pub struct Resources {
    files: Vec<Resource>,
    by_url: BTreeMap<(String, bool), Result<usize, Refused>>,
}

impl Resources {
    /// The path of the file behind `url`, loading it the first time
    /// it is asked for.
    pub fn resolve(
        &mut self,
        url: &str,
        kind: Kind,
        load: impl FnOnce(&str) -> Option<Vec<u8>>,
    ) -> Result<&str, Refused> {
        let key = (url.to_string(), kind == Kind::Font);
        if !self.by_url.contains_key(&key) {
            let found = match load(url) {
                None => Err(Refused::Missing),
                Some(bytes) => match media_type(&bytes, kind) {
                    None => Err(Refused::Unknown),
                    Some((media_type, extension)) => {
                        let stem = match kind {
                            Kind::Image => "image",
                            Kind::Font => "font",
                        };
                        let number = self.files.len() + 1;
                        self.files.push(Resource {
                            href: format!("media/{stem}-{number}.{extension}"),
                            media_type,
                            bytes,
                        });
                        Ok(self.files.len() - 1)
                    }
                },
            };
            self.by_url.insert(key.clone(), found);
        }
        match self.by_url[&key] {
            Ok(index) => Ok(&self.files[index].href),
            Err(refused) => Err(refused),
        }
    }

    /// Every file, in the order it was first asked for.
    pub fn files(&self) -> &[Resource] {
        &self.files
    }

    /// The same, taken out of the table.
    pub fn into_files(self) -> Vec<Resource> {
        self.files
    }
}

/// The media type and a file extension for `bytes`, read from their
/// first bytes. Only the types an EPUB holds without a fallback.
fn media_type(bytes: &[u8], kind: Kind) -> Option<(&'static str, &'static str)> {
    match kind {
        Kind::Image => {
            if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                Some(("image/png", "png"))
            } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                Some(("image/jpeg", "jpg"))
            } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
                Some(("image/gif", "gif"))
            } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
                Some(("image/webp", "webp"))
            } else if is_svg(bytes) {
                Some(("image/svg+xml", "svg"))
            } else {
                None
            }
        }
        Kind::Font => match bytes.get(0..4)? {
            [0, 1, 0, 0] | b"true" => Some(("font/ttf", "ttf")),
            b"OTTO" => Some(("font/otf", "otf")),
            b"wOFF" => Some(("font/woff", "woff")),
            b"wOF2" => Some(("font/woff2", "woff2")),
            _ => None,
        },
    }
}

/// An SVG file opens with its root element, perhaps after an XML
/// declaration, a doctype or a comment.
fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let Ok(text) = std::str::from_utf8(head) else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    (text.starts_with('<')) && text.contains("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    /// A url asked for twice is one file, loaded once.
    #[test]
    fn a_url_is_loaded_once_and_named_in_order() {
        let mut resources = Resources::default();
        let mut loads = 0;
        let mut load = |_: &str| {
            loads += 1;
            Some(PNG.to_vec())
        };
        assert_eq!(
            resources.resolve("a.png", Kind::Image, &mut load),
            Ok("media/image-1.png")
        );
        assert_eq!(
            resources.resolve("b.png", Kind::Image, &mut load),
            Ok("media/image-2.png")
        );
        assert_eq!(
            resources.resolve("a.png", Kind::Image, &mut load),
            Ok("media/image-1.png")
        );
        assert_eq!(loads, 2);
        assert_eq!(resources.files()[0].media_type, "image/png");
    }

    #[test]
    fn a_file_the_host_lacks_or_the_format_cannot_hold_is_refused() {
        let mut resources = Resources::default();
        assert_eq!(
            resources.resolve("gone.png", Kind::Image, |_| None),
            Err(Refused::Missing)
        );
        assert_eq!(
            resources.resolve("a.bmp", Kind::Image, |_| Some(b"BM....".to_vec())),
            Err(Refused::Unknown)
        );
        // A font is not an image, even at the same url.
        assert_eq!(
            resources.resolve("a.png", Kind::Font, |_| Some(PNG.to_vec())),
            Err(Refused::Unknown)
        );
        assert!(resources.files().is_empty());
    }

    #[test]
    fn the_first_bytes_name_the_type() {
        let cases: [(&[u8], Kind, &str); 8] = [
            (PNG, Kind::Image, "image/png"),
            (&[0xFF, 0xD8, 0xFF, 0xE0], Kind::Image, "image/jpeg"),
            (b"GIF89a..", Kind::Image, "image/gif"),
            (b"RIFF\0\0\0\0WEBPVP8 ", Kind::Image, "image/webp"),
            (
                b"<?xml version=\"1.0\"?>\n<svg xmlns=\"\">",
                Kind::Image,
                "image/svg+xml",
            ),
            (&[0, 1, 0, 0, 0, 9], Kind::Font, "font/ttf"),
            (b"OTTO\0\0", Kind::Font, "font/otf"),
            (b"wOF2\0\0", Kind::Font, "font/woff2"),
        ];
        for (bytes, kind, expected) in cases {
            assert_eq!(media_type(bytes, kind).map(|(t, _)| t), Some(expected));
        }
    }
}
