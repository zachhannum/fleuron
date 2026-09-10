//! The fixtures the session tests are written against: books to lay
//! out, sheets to lay them out under, and the paint ops one run came
//! to.

use crate::LayoutOutput;
use crate::content::{Attributes, Block, Book, HeadingLevel, Inline, Metadata, NodeId, Section};
use crate::fonts::FontRegistry;
use crate::pages::DrawItem;
use crate::style::{Source, Stylesheets};

use super::Session;

pub(super) fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| crate::fonts::bundled_registry().expect("bundled font parses"))
}

/// The fixture map: a JPEG the layout pass sizes from its header.
pub(super) const MAP: &[u8] = include_bytes!("../../../../fixtures/images/plate.jpg");

/// A GIF's minimal header: the signature and a logical screen
/// descriptor, which is all `probe` reads for the format.
/// Anything after it is ignored, which is what lets two of these
/// with the same declared size still hash to different bytes.
pub(super) fn gif(width: u16, height: u16, tag: u8) -> Vec<u8> {
    let mut bytes = b"GIF89a".to_vec();
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    bytes.push(tag);
    bytes
}

/// A book with one image, for the replacement tests below.
pub(super) fn book_with_image(url: &str) -> Book {
    let mut book = Book {
        sections: vec![Section {
            blocks: vec![Block::Image {
                id: NodeId::UNASSIGNED,
                url: url.into(),
                alt: "an image".into(),
                attributes: Attributes::default(),
                position: None,
                span: None,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    book.assign_node_ids();
    book
}

/// The size the first placed image on the page reports, if there
/// is one.
pub(super) fn placed_image_size(output: &LayoutOutput) -> Option<(f32, f32)> {
    output
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .find_map(|item| match item {
            DrawItem::Image { w, h, .. } => Some((*w, *h)),
            _ => None,
        })
}

/// An RGBA PNG one inch square, opaque on its left half.
pub(super) fn alpha_png(tint: u8) -> Vec<u8> {
    let side = 96u32;
    let mut pixels = Vec::with_capacity((side * side * 4) as usize);
    for _ in 0..side {
        for x in 0..side {
            pixels.extend([tint, 0x33, 0x44, if x < side / 2 { 0xFF } else { 0 }]);
        }
    }
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("the header writes");
    writer.write_image_data(&pixels).expect("the pixels write");
    writer.finish().expect("the file closes");
    bytes
}

/// A session over one book with one image in it, and the alpha to
/// trace it from.
pub(super) fn illustrated() -> Session<'static> {
    let mut session = Session::owning(crate::fonts::bundled_registry().unwrap());
    session.set_content(book_with_image("plate.png"));
    session.add_image("plate.png", alpha_png(0x22)).unwrap();
    session
}

pub(super) fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

pub(super) fn paragraph(value: &str) -> Block {
    Block::Paragraph {
        id: NodeId::UNASSIGNED,
        inlines: vec![text(value)],
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

pub(super) fn heading(value: &str) -> Block {
    Block::Heading {
        id: NodeId::UNASSIGNED,
        level: HeadingLevel::H1,
        inlines: vec![text(value)],
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

/// Paragraphs of one word repeated, so that every line of a
/// section repeats its tag and can be followed across an edit.
pub(super) fn prose(tag: &str, paragraphs: usize) -> Vec<Block> {
    (0..paragraphs)
        .map(|_| paragraph(&format!("{tag} ").repeat(80)))
        .collect()
}

pub(super) fn section(source: &str, blocks: Vec<Block>) -> Section {
    Section {
        id: NodeId::UNASSIGNED,
        source: Some(source.into()),
        title: None,
        blocks,
        position: None,
        span: None,
    }
}

pub(super) fn book(sections: Vec<Section>) -> Book {
    Book {
        metadata: Metadata::default(),
        sections,
    }
}

pub(super) fn sheets(css: &str) -> Stylesheets {
    Stylesheets::parse(&[Source::author("test.css", css)])
}

/// Where a node is named directly, walked the way a host would
/// have to walk it: the pages a run of that node is set on, and
/// the pages that name it as a section, by their place in the
/// book.
pub(super) fn named_on(output: &LayoutOutput, node: NodeId) -> Vec<usize> {
    output
        .pages
        .iter()
        .enumerate()
        .filter(|(_, page)| {
            page.sections.contains(&node)
                || page.items.iter().any(|item| {
                    matches!(item, DrawItem::Text { origin: Some(origin), .. }
                        if origin.node == node)
                })
        })
        .map(|(at, _)| at)
        .collect()
}

/// A session over three chapters, one file each.
pub(super) fn three_chapters() -> Session<'static> {
    let mut session = Session::new(registry());
    session.set_content(book(vec![
        section("one.md", prose("alpha", 8)),
        section("two.md", prose("beta", 8)),
        section("three.md", prose("gamma", 8)),
    ]));
    session.preview();
    session
}

/// A book of one paragraph, declaring `language` or declaring
/// none, on a measure narrow enough for hyphenation to decide
/// where its lines end.
pub(super) fn hyphenated(language: Option<&str>, prose: &str) -> Session<'static> {
    let mut session = Session::new(registry());
    session.set_style(sheets(
        "@page { size: 45pt 400pt; margin: 12pt } p { hyphens: auto }",
    ));
    session.set_content(Book {
        metadata: language.map(declaring).unwrap_or_default(),
        sections: vec![section("one.md", vec![paragraph(prose)])],
    });
    session
}

/// The book's declared language, and nothing else named.
pub(super) fn declaring(tag: &str) -> Metadata {
    Metadata {
        extra: [("language".to_string(), tag.to_string())]
            .into_iter()
            .collect(),
        ..Default::default()
    }
}

/// Every painted run, in the order the pages carry them.
pub(super) fn painted(output: &LayoutOutput) -> Vec<String> {
    output
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Every text run containing `tag`, as `(page, x, baseline, text)`.
pub(super) fn runs(output: &LayoutOutput, tag: &str) -> Vec<(usize, u32, u32, String)> {
    let mut found = Vec::new();
    for (index, page) in output.pages.iter().enumerate() {
        for item in &page.items {
            if let DrawItem::Text { x, y, text, .. } = item
                && text.contains(tag)
            {
                found.push((index, x.to_bits(), y.to_bits(), text.clone()));
            }
        }
    }
    found
}

pub(super) fn spelled(runs: &[(usize, u32, u32, String)]) -> Vec<&str> {
    runs.iter().map(|(_, _, _, text)| text.as_str()).collect()
}

pub(super) fn placed(runs: &[(usize, u32, u32, String)]) -> Vec<(usize, u32, u32)> {
    runs.iter().map(|(page, x, y, _)| (*page, *x, *y)).collect()
}
