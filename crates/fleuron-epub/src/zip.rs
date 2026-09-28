//! The container: a zip archive with every timestamp pinned, so one
//! book always makes the same bytes.

/// 1980-01-01 00:00:00, the earliest time a zip entry can carry.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

/// Version 2.0: deflate, and no feature past it.
const VERSION: u16 = 20;

/// Bit 11: the entry name is UTF-8.
const UTF8: u16 = 1 << 11;

/// How an entry's bytes are held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// As they are. The `mimetype` entry has to be held this way.
    Stored,
    /// Deflated.
    Deflated,
}

impl Method {
    fn code(self) -> u16 {
        match self {
            Method::Stored => 0,
            Method::Deflated => 8,
        }
    }
}

/// One entry already in the archive, as the central directory lists
/// it.
struct Entry {
    name: String,
    method: Method,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

/// An archive being written, entry by entry, in the order the entries
/// are added.
#[derive(Default)]
pub struct Archive {
    bytes: Vec<u8>,
    entries: Vec<Entry>,
}

impl Archive {
    /// Adds one entry.
    pub fn add(&mut self, name: &str, data: &[u8], method: Method) {
        let held = match method {
            Method::Stored => data.to_vec(),
            Method::Deflated => miniz_oxide::deflate::compress_to_vec(data, 9),
        };
        let entry = Entry {
            name: name.to_string(),
            method,
            crc: crc32fast::hash(data),
            compressed: held.len() as u32,
            size: data.len() as u32,
            offset: self.bytes.len() as u32,
        };
        let out = &mut self.bytes;
        u32le(out, 0x0403_4b50);
        u16le(out, VERSION);
        header(out, &entry);
        u16le(out, 0);
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&held);
        self.entries.push(entry);
    }

    /// The archive, with its central directory written after the
    /// entries.
    pub fn finish(mut self) -> Vec<u8> {
        let start = self.bytes.len() as u32;
        let out = &mut self.bytes;
        for entry in &self.entries {
            u32le(out, 0x0201_4b50);
            u16le(out, VERSION);
            u16le(out, VERSION);
            header(out, entry);
            // Extra field, comment, disk, internal and external
            // attributes.
            u16le(out, 0);
            u16le(out, 0);
            u16le(out, 0);
            u16le(out, 0);
            u32le(out, 0);
            u32le(out, entry.offset);
            out.extend_from_slice(entry.name.as_bytes());
        }
        let size = out.len() as u32 - start;
        let count = self.entries.len() as u16;
        u32le(out, 0x0605_4b50);
        u16le(out, 0);
        u16le(out, 0);
        u16le(out, count);
        u16le(out, count);
        u32le(out, size);
        u32le(out, start);
        u16le(out, 0);
        self.bytes
    }
}

/// The fields a local header and a central directory entry share,
/// from the flags to the length of the name.
fn header(out: &mut Vec<u8>, entry: &Entry) {
    let flags = if entry.name.is_ascii() { 0 } else { UTF8 };
    u16le(out, flags);
    u16le(out, entry.method.code());
    u16le(out, DOS_TIME);
    u16le(out, DOS_DATE);
    u32le(out, entry.crc);
    u32le(out, entry.compressed);
    u32le(out, entry.size);
    u16le(out, entry.name.len() as u16);
}

fn u16le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive() -> Vec<u8> {
        let mut archive = Archive::default();
        archive.add("mimetype", b"application/epub+zip", Method::Stored);
        archive.add(
            "EPUB/a.txt",
            "a line of prose\n".repeat(40).as_bytes(),
            Method::Deflated,
        );
        archive.finish()
    }

    /// The first entry is the media type, held as it is, so the
    /// bytes at offset 38 name the format.
    #[test]
    fn the_first_entry_is_stored_and_readable_at_a_fixed_offset() {
        let bytes = archive();
        assert_eq!(&bytes[0..4], b"PK\x03\x04");
        assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 0, "stored");
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..58], b"application/epub+zip");
    }

    /// Every entry carries the same date, so two runs match.
    #[test]
    fn two_archives_of_the_same_entries_are_the_same_bytes() {
        assert_eq!(archive(), archive());
        let bytes = archive();
        assert_eq!(u16::from_le_bytes([bytes[12], bytes[13]]), DOS_DATE);
    }

    /// A deflated entry inflates back to what went in.
    #[test]
    fn a_deflated_entry_inflates_to_its_data() {
        let bytes = archive();
        let second = 38 + 20;
        assert_eq!(&bytes[second..second + 4], b"PK\x03\x04");
        let compressed = u32::from_le_bytes(bytes[second + 18..second + 22].try_into().unwrap());
        let data = second + 30 + "EPUB/a.txt".len();
        let inflated =
            miniz_oxide::inflate::decompress_to_vec(&bytes[data..data + compressed as usize])
                .unwrap();
        assert_eq!(inflated, "a line of prose\n".repeat(40).as_bytes());
    }
}
