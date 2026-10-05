//! Image sizing, and the contour a sheet asks to be traced.
//!
//! Layout needs one thing from an image — how big it is — and getting
//! it by decoding would put a pixel buffer in the layout pass. So the
//! engine reads the header: PNG's `IHDR` and `pHYs`, JPEG's `SOFn` and
//! JFIF density, GIF's screen descriptor, WebP's chunk headers, and
//! the `width`, `height` and `viewBox` of an SVG's root element. The
//! file is kept as it arrived, and a painter is what decodes it.
//!
//! A url reaches the table from two places. The content tree names
//! one for every image the manuscript places, and the style tree
//! names one for every box the sheet puts art behind, so probing
//! takes both.
//!
//! One thing else decodes it: `shape-outside: auto`, which sets prose
//! around the shape of an image rather than around its box. That is
//! not reachable from where probing happens, because probing has not
//! seen the style tree, so it is a stage of its own. [`Contours`] is
//! what that stage keeps: it walks the cascade for the nodes that ask
//! for a traced contour, and traces each asset once.
//!
//! The engine opens nothing itself, the same as with fonts. Whatever string
//! the content tree writes is the name an image is matched under. It
//! never has to be a real URL; the host is what turns a name into
//! bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::Warning;
use crate::content::{Block, Book};
use crate::style::{ComputedStyle, ShapeOutside, StyleTree, Url};

/// Resolves image urls to bytes.
///
/// The whole file, not a prefix: the header sizes the box and the
/// same bytes are what a painter embeds.
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

/// The images a host already handed over answer by the url they
/// were registered under, with the bytes as they arrived.
impl ImageLoader for Assets {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        let (index, _) = self.lookup(url)?;
        self.bytes(index).map(<[u8]>::to_vec)
    }
}

/// What CSS calls one pixel: 1/96th of an inch. An image whose header
/// declares no resolution is measured at this one.
pub const CSS_DPI: f32 = 96.0;

/// An image's own idea of its size: pixels, and the resolution they
/// are meant to be shown at.
///
/// A vector image has no pixels. Its `width` and `height` are its
/// size in CSS pixels to the nearest whole one, and the resolution
/// carries the remainder, so [`Intrinsic::size`] is exact.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Intrinsic {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Horizontal resolution in pixels per inch.
    pub dpi_x: f32,
    /// Vertical resolution in pixels per inch.
    pub dpi_y: f32,
    /// Whether the file gives a size. False for a vector image that
    /// gives a ratio alone: `width` and `height` are then the two
    /// sides of the ratio, and what sizes the image is the box it is
    /// placed in.
    pub sized: bool,
}

impl Intrinsic {
    /// The intrinsic size in points, at the header's own resolution.
    /// For an image with a ratio and no size, the two sides of the
    /// ratio.
    pub fn size(self) -> (f32, f32) {
        (
            self.width as f32 / self.dpi_x * 72.0,
            self.height as f32 / self.dpi_y * 72.0,
        )
    }

    /// The size the image takes where no sheet gives it one, in a
    /// box `measure` wide. An image with a ratio and no size fills
    /// the width, as CSS has a replaced element do.
    pub fn size_within(self, measure: f32) -> (f32, f32) {
        let (width, height) = self.size();
        if self.sized || width <= 0.0 {
            return (width, height);
        }
        (measure, measure * height / width)
    }
}

/// One image the book refers to, sized from its header.
///
/// What `DrawItem::Image.asset` indexes, and all the display structure
/// says about an image: painters take the url back to their own
/// pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// The file each asset was probed from, in the same order.
    /// Layout read the header out of these; the PDF writer embeds
    /// them.
    files: Vec<Vec<u8>>,
    /// A hash of each file, in the same order: what tells a url
    /// registered again with the same bytes from one registered
    /// again with different ones.
    hashes: Vec<u64>,
    /// Urls that were offered and could not be sized, so that a url
    /// nothing has ever answered for can be told apart from one
    /// already complained about.
    refused: BTreeSet<String>,
    warnings: Vec<Warning>,
}

/// What registering an image did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Added {
    /// The bytes already registered at this url, unchanged.
    Unchanged(u32),
    /// New bytes at this index: the url's first registration, or
    /// different bytes replacing what was there. `previous` is the
    /// intrinsic size that answered for it before, `None` for a
    /// fresh url — what a session reads to decide whether the box an
    /// image takes moved.
    Replaced {
        /// The index the bytes answer for.
        index: u32,
        /// What the same index reported before, if anything did.
        previous: Option<Intrinsic>,
        /// What it reports now.
        current: Intrinsic,
    },
    /// The bytes did not probe: no size could be read from them.
    Refused,
}

/// What one file hashes to, for telling a registration of the same
/// bytes from one of different bytes at the same url.
fn content_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

impl Assets {
    /// No images at all: what a host with no loader supplies.
    pub fn none() -> Assets {
        Assets::default()
    }

    /// Probes every image the book places and every image its sheet
    /// puts behind a box, the manuscript's own first. A url the
    /// loader cannot resolve, or bytes no probe recognises, is a
    /// diagnostic and no asset.
    ///
    /// A sheet that names no background image offers the loader
    /// nothing beyond what the manuscript named.
    pub fn probe(book: &Book, styles: &StyleTree, loader: &dyn ImageLoader) -> Assets {
        let mut assets = Assets::default();
        for section in &book.sections {
            assets.walk(&section.blocks, loader);
        }
        for background in backgrounds(styles) {
            assets.take(background, loader);
        }
        assets
    }

    /// Registers one image the host handed over, and what that did:
    /// the same bytes it already had, new bytes at a fresh or
    /// existing index, or bytes no probe recognises.
    ///
    /// This is the door for a host that pushes: a worker has no
    /// loader to reach back through, so images cross the wall the
    /// way font files do. A url pushed again with the bytes it
    /// already answers for costs nothing; pushed again with
    /// different bytes, it replaces them in place, keeping the index
    /// `DrawItem::Image.asset` already names.
    pub fn add(&mut self, url: &str, bytes: Vec<u8>) -> Added {
        let hash = content_hash(&bytes);
        if let Some(index) = self.assets.iter().position(|asset| asset.url == url) {
            if self.hashes[index] == hash {
                return Added::Unchanged(index as u32);
            }
            return match probe(&bytes) {
                Some(intrinsic) => {
                    let previous = self.assets[index].intrinsic;
                    self.assets[index] = Asset {
                        url: url.to_string(),
                        intrinsic,
                    };
                    self.files[index] = bytes;
                    self.hashes[index] = hash;
                    self.refused.remove(url);
                    Added::Replaced {
                        index: index as u32,
                        previous: Some(previous),
                        current: intrinsic,
                    }
                }
                None => {
                    self.refuse(url, None);
                    Added::Refused
                }
            };
        }
        match probe(&bytes) {
            Some(intrinsic) => {
                self.refused.remove(url);
                self.assets.push(Asset {
                    url: url.to_string(),
                    intrinsic,
                });
                self.files.push(bytes);
                self.hashes.push(hash);
                Added::Replaced {
                    index: self.assets.len() as u32 - 1,
                    previous: None,
                    current: intrinsic,
                }
            }
            None => {
                self.refuse(url, None);
                Added::Refused
            }
        }
    }

    /// The asset registered for a url, and its index.
    pub fn lookup(&self, url: &str) -> Option<(u32, Intrinsic)> {
        self.assets
            .iter()
            .position(|asset| asset.url == url)
            .map(|index| (index as u32, self.assets[index].intrinsic))
    }

    /// Whether a url has been probed: an asset came of it, or a
    /// complaint about why none did. A url nobody offered is neither,
    /// and is what layout reports.
    pub fn probed(&self, url: &str) -> bool {
        self.refused.contains(url) || self.lookup(url).is_some()
    }

    /// Every asset, in the order `DrawItem::Image.asset` indexes them.
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    /// The file one asset was probed from: the bytes a painter
    /// embeds, unchanged from what the host handed over.
    pub fn bytes(&self, index: u32) -> Option<&[u8]> {
        self.files.get(index as usize).map(Vec::as_slice)
    }

    /// What one asset's file hashes to: what tells a trace of the
    /// bytes an index holds now from a trace of the bytes it held
    /// before.
    pub fn digest(&self, index: u32) -> Option<u64> {
        self.hashes.get(index as usize).copied()
    }

    /// What probing had to complain about.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    fn refuse(&mut self, url: &str, origin: Option<String>) {
        if self.refused.insert(url.to_string()) {
            self.warnings.push(Warning {
                message: format!("Image {url} has no size in it. The image is skipped."),
                origin,
            });
        }
    }

    /// Probes one url the sheet named, at the position it was
    /// written.
    fn take(&mut self, url: &Url, loader: &dyn ImageLoader) {
        if self.probed(&url.value) {
            return;
        }
        match loader.load(&url.value) {
            Some(bytes) => match probe(&bytes) {
                Some(intrinsic) => {
                    self.assets.push(Asset {
                        url: url.value.clone(),
                        intrinsic,
                    });
                    self.hashes.push(content_hash(&bytes));
                    self.files.push(bytes);
                }
                None => self.refuse(&url.value, url.origin.clone()),
            },
            None => self.refuse(&url.value, url.origin.clone()),
        }
    }

    fn walk(&mut self, blocks: &[Block], loader: &dyn ImageLoader) {
        for block in blocks {
            match block {
                Block::Image { url, position, .. } => {
                    if self.probed(url) {
                        continue;
                    }
                    let origin = Some(crate::content::origin(None, *position));
                    match loader.load(url) {
                        Some(bytes) => match probe(&bytes) {
                            Some(intrinsic) => {
                                self.assets.push(Asset {
                                    url: url.clone(),
                                    intrinsic,
                                });
                                self.hashes.push(content_hash(&bytes));
                                self.files.push(bytes);
                            }
                            None => self.refuse(url, origin),
                        },
                        None => self.refuse(url, origin),
                    }
                }
                Block::Blockquote { blocks, .. } => self.walk(blocks, loader),
                Block::Table { head, body, .. } => {
                    for blocks in crate::content::cell_blocks(head, body) {
                        self.walk(blocks, loader);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Every image a book's sheet puts behind a box: one per block style
/// and one per page master, in a stable order.
pub fn backgrounds(styles: &StyleTree) -> Vec<&Url> {
    styles
        .styles()
        .iter()
        .filter_map(|style| style.background.image.as_ref())
        .chain(
            styles
                .masters()
                .iter()
                .filter_map(|master| master.style.background.image.as_ref()),
        )
        .collect()
}

/// Every contour a book's sheet asked for, traced once and kept by
/// asset.
///
/// This is the trace stage. Nothing is traced because it is an image.
/// It is traced because a rule that matched it resolved to
/// `shape-outside: auto`, and that is not reachable from where
/// probing happens: probing takes a book and a loader, and neither
/// has seen the style tree.
///
/// The stage sits above line breaking, because a contour that moved
/// is a measure that moved. It keeps what an earlier run traced, so a
/// sheet edit that leaves the contours where they were decodes
/// nothing.
#[derive(Debug, Default)]
pub struct Contours {
    traced: BTreeMap<u32, Traced>,
    warned: BTreeSet<String>,
    warnings: Vec<Warning>,
}

/// Whether one node's styling asks for a contour the flow can read.
///
/// Both halves matter. A sheet that names `auto` on an element it
/// leaves in the flow asks for a contour nothing sets beside, and an
/// image is not decoded to answer that.
fn traceable(style: &ComputedStyle) -> bool {
    style.shape_outside == ShapeOutside::Auto && style.excludes()
}

/// One asset's trace, and the bytes it was traced from.
#[derive(Debug)]
struct Traced {
    digest: u64,
    contour: Option<Contour>,
}

impl Contours {
    /// Nothing traced: what a book whose sheet names no contour
    /// carries.
    pub fn none() -> Contours {
        Contours::default()
    }

    /// Traces every asset the cascade asks for and this table does not
    /// already answer for, and reports how many images it decoded.
    ///
    /// A book whose sheet names no contour decodes nothing, and so
    /// does one whose contours are all traced already.
    pub fn update(&mut self, book: &Book, styles: &StyleTree, assets: &Assets) -> u32 {
        fn walk(
            contours: &mut Contours,
            blocks: &[Block],
            source: Option<&str>,
            styles: &StyleTree,
            assets: &Assets,
            traced: &mut u32,
        ) {
            for block in blocks {
                match block {
                    Block::Blockquote { blocks, .. } => {
                        walk(contours, blocks, source, styles, assets, traced)
                    }
                    Block::Table { head, body, .. } => {
                        for blocks in crate::content::cell_blocks(head, body) {
                            walk(contours, blocks, source, styles, assets, traced);
                        }
                    }
                    Block::Image {
                        id, url, position, ..
                    } if traceable(styles.style(*id)) => {
                        let Some(((index, _), digest)) = assets
                            .lookup(url)
                            .and_then(|found| Some((found, assets.digest(found.0)?)))
                        else {
                            continue;
                        };
                        if contours
                            .traced
                            .get(&index)
                            .is_some_and(|kept| kept.digest == digest)
                        {
                            continue;
                        }
                        let contour = assets.bytes(index).and_then(trace);
                        *traced += 1;
                        if contour.is_none() && contours.warned.insert(url.clone()) {
                            let origin = crate::content::origin(source, *position);
                            contours.warnings.push(Warning {
                                message: format!(
                                    "Missing alpha channel in {url}. Text wraps around the \
                                     image rectangle."
                                ),
                                origin: (!origin.is_empty()).then_some(origin),
                            });
                        }
                        contours.traced.insert(index, Traced { digest, contour });
                    }
                    _ => {}
                }
            }
        }
        let mut traced = 0;
        for section in &book.sections {
            walk(
                self,
                &section.blocks,
                section.source.as_deref(),
                styles,
                assets,
                &mut traced,
            );
        }
        traced
    }

    /// The contour one asset traced to, and `None` where it traced to
    /// nothing or was never asked for.
    pub fn get(&self, asset: u32) -> Option<&Contour> {
        self.traced.get(&asset)?.contour.as_ref()
    }

    /// Whether this table holds a trace of one asset, whatever the
    /// trace found.
    pub fn traces(&self, asset: u32) -> bool {
        self.traced.contains_key(&asset)
    }

    /// What tracing had to complain about.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }
}

/// The outline prose sets around, in the image's own box.
///
/// A ring is a closed loop of points, `(0, 0)` at the top left of the
/// image and `(1, 1)` at its bottom right. An image whose alpha
/// leaves two shapes with clear space between them traces to two
/// rings, and the prose sets through the space.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    /// The rings, each closed by its own first point.
    pub rings: Vec<Vec<[f32; 2]>>,
}

/// How many rows a trace reports, so that the outline one image
/// traces to is the same size whatever the image's pixel count is.
const ROWS: u32 = 128;

/// The largest image the tracer decodes. A file above this contributes
/// its box, because the buffer a decode wants is the picture again in
/// memory and a contour is not worth that.
const CEILING: u64 = 32 * 1024 * 1024;

/// The alpha channel of one image, one byte to a pixel.
struct Mask {
    width: u32,
    height: u32,
    alpha: Vec<u8>,
}

impl Mask {
    /// The first and last covered pixel of every row, `None` for a
    /// row nothing covers.
    fn rows(&self) -> Vec<Option<(u32, u32)>> {
        (0..self.height)
            .map(|y| {
                let row = &self.alpha[(y * self.width) as usize..][..self.width as usize];
                let first = row.iter().position(|alpha| *alpha > 0)? as u32;
                let last = row.iter().rposition(|alpha| *alpha > 0)? as u32;
                Some((first, last + 1))
            })
            .collect()
    }
}

/// Traces one image's alpha channel. `None` where the format carries
/// no alpha, where the file does not decode, or where it is larger
/// than the tracer decodes.
///
/// The outline is reported over a fixed number of bands rather than
/// over the image's own rows, so two images of the same shape and
/// different resolutions trace to the same size of outline.
pub fn trace(bytes: &[u8]) -> Option<Contour> {
    let mask = mask(bytes)?;
    let rows = mask.rows();
    let bands: Vec<Option<(f32, f32)>> = (0..ROWS)
        .map(|band| {
            let from = (band as u64 * mask.height as u64 / ROWS as u64) as usize;
            let to = ((band as u64 + 1) * mask.height as u64 / ROWS as u64).max(from as u64 + 1);
            rows.get(from..(to as usize).min(rows.len()))?
                .iter()
                .flatten()
                .copied()
                .reduce(|(left, right), (start, end)| (left.min(start), right.max(end)))
                .map(|(left, right)| {
                    (
                        left as f32 / mask.width as f32,
                        right as f32 / mask.width as f32,
                    )
                })
        })
        .collect();
    Some(Contour {
        rings: rings(&bands),
    })
}

/// One ring for every run of bands the alpha covers, in reading
/// order. A band nothing covers ends the ring above it, which is what
/// lets prose set through a gap in the image.
fn rings(bands: &[Option<(f32, f32)>]) -> Vec<Vec<[f32; 2]>> {
    let mut rings = Vec::new();
    let mut run: Vec<(usize, (f32, f32))> = Vec::new();
    let height = bands.len() as f32;
    let close = |run: &mut Vec<(usize, (f32, f32))>, rings: &mut Vec<Vec<[f32; 2]>>| {
        if run.is_empty() {
            return;
        }
        let edge = |side: fn(&(f32, f32)) -> f32, run: &[(usize, (f32, f32))]| {
            let mut points: Vec<[f32; 2]> = Vec::new();
            for (band, span) in run {
                let (top, bottom) = (*band as f32 / height, (*band + 1) as f32 / height);
                let x = side(span);
                // A band no wider than the one above it needs no
                // corner of its own.
                match points.last_mut() {
                    Some(last) if last[0] == x => last[1] = bottom,
                    _ => {
                        points.push([x, top]);
                        points.push([x, bottom]);
                    }
                }
            }
            points
        };
        let mut ring = edge(|span| span.0, run);
        let mut right = edge(|span| span.1, run);
        right.reverse();
        ring.append(&mut right);
        rings.push(ring);
        run.clear();
    };
    for (band, span) in bands.iter().enumerate() {
        match span {
            Some(span) => run.push((band, *span)),
            None => close(&mut run, &mut rings),
        }
    }
    close(&mut run, &mut rings);
    rings
}

/// The alpha channel of one file, for the formats that carry one.
fn mask(bytes: &[u8]) -> Option<Mask> {
    let intrinsic = probe(bytes)?;
    if intrinsic.width as u64 * intrinsic.height as u64 > CEILING {
        return None;
    }
    png_mask(bytes)
        .or_else(|| webp_mask(bytes))
        .or_else(|| gif_mask(bytes))
}

/// PNG. `normalize_to_color8` expands a palette and a `tRNS` chunk,
/// so an alpha channel arrives however the file wrote it.
fn png_mask(bytes: &[u8]) -> Option<Mask> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let (channels, offset) = match reader.output_color_type() {
        (png::ColorType::Rgba, _) => (4, 3),
        (png::ColorType::GrayscaleAlpha, _) => (2, 1),
        _ => return None,
    };
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    Some(Mask {
        width: info.width,
        height: info.height,
        alpha: buffer[..(info.width * info.height * channels) as usize]
            .iter()
            .skip(offset)
            .step_by(channels as usize)
            .copied()
            .collect(),
    })
}

/// WebP, when the file declares an alpha channel.
fn webp_mask(bytes: &[u8]) -> Option<Mask> {
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    if !decoder.has_alpha() {
        return None;
    }
    let (width, height) = decoder.dimensions();
    let mut buffer = vec![0u8; decoder.output_buffer_size()?];
    decoder.read_image(&mut buffer).ok()?;
    Some(Mask {
        width,
        height,
        alpha: buffer.iter().skip(3).step_by(4).copied().collect(),
    })
}

/// GIF, whose transparency is one palette entry. The first frame is
/// the image, and a frame smaller than the screen leaves the rest of
/// it clear.
fn gif_mask(bytes: &[u8]) -> Option<Mask> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).ok()?;
    let (width, height) = (decoder.width() as u32, decoder.height() as u32);
    let frame = decoder.read_next_frame().ok()??;
    let mut alpha = vec![0u8; (width * height) as usize];
    for y in 0..frame.height as u32 {
        for x in 0..frame.width as u32 {
            let (at_x, at_y) = (x + frame.left as u32, y + frame.top as u32);
            if at_x >= width || at_y >= height {
                continue;
            }
            alpha[(at_y * width + at_x) as usize] =
                frame.buffer[((y * frame.width as u32 + x) * 4 + 3) as usize];
        }
    }
    Some(Mask {
        width,
        height,
        alpha,
    })
}

/// Reads an image's size from its header. `None` for a format no
/// probe recognises, or a header too short to read one from.
pub fn probe(bytes: &[u8]) -> Option<Intrinsic> {
    png(bytes)
        .or_else(|| jpeg(bytes))
        .or_else(|| gif(bytes))
        .or_else(|| webp(bytes))
        .or_else(|| svg(bytes))
}

/// Whether a file is an SVG: its root element is `svg`.
pub fn is_svg(bytes: &[u8]) -> bool {
    svg_root(bytes).is_some()
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
        sized: true,
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
        // Padding fill bytes, and the standalone markers with no
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
                sized: true,
            });
        }
        if marker == 0xDA {
            return None;
        }
        at = at.checked_add(2)?.checked_add(length)?;
    }
}

/// GIF: the logical screen descriptor, which has no resolution in it.
fn gif(bytes: &[u8]) -> Option<Intrinsic> {
    if !bytes.starts_with(b"GIF87a") && !bytes.starts_with(b"GIF89a") {
        return None;
    }
    Some(Intrinsic {
        width: le16(bytes, 6)? as u32,
        height: le16(bytes, 8)? as u32,
        dpi_x: CSS_DPI,
        dpi_y: CSS_DPI,
        sized: true,
    })
}

/// WebP: the extended header when there is one, the lossy or lossless
/// frame header otherwise. None of the three records a resolution.
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
            sized: true,
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

/// SVG: the root element's `width` and `height` where they are
/// absolute lengths, and its `viewBox` for a side they leave out. A
/// root with a `viewBox` and no size gives a ratio alone.
fn svg(bytes: &[u8]) -> Option<Intrinsic> {
    let root = svg_root(bytes)?;
    let width = attribute(root, "width").and_then(svg_length);
    let height = attribute(root, "height").and_then(svg_length);
    let view = attribute(root, "viewBox").and_then(view_box);
    match (width, height, view) {
        (Some(width), Some(height), _) => vector(width, height, true),
        (Some(width), None, Some((across, down))) => vector(width, width * down / across, true),
        (None, Some(height), Some((across, down))) => vector(height * across / down, height, true),
        (None, None, Some((across, down))) => vector(across, down, false),
        _ => None,
    }
}

/// A size in CSS pixels as an [`Intrinsic`]. The pixel count is the
/// nearest whole one, and the resolution makes up the difference.
fn vector(width: f32, height: f32, sized: bool) -> Option<Intrinsic> {
    let whole = |side: f32| side.round().max(1.0);
    let finite = |side: f32| side.is_finite() && side > 0.0;
    (finite(width) && finite(height)).then(|| Intrinsic {
        width: whole(width) as u32,
        height: whole(height) as u32,
        dpi_x: CSS_DPI * whole(width) / width,
        dpi_y: CSS_DPI * whole(height) / height,
        sized,
    })
}

/// The attributes of the root element, where that element is `svg`.
/// An XML declaration, a doctype and comments may come before it.
fn svg_root(bytes: &[u8]) -> Option<&str> {
    let after = |rest: &[u8], close: &[u8]| -> Option<usize> {
        rest.windows(close.len())
            .position(|window| window == close)
            .map(|at| at + close.len())
    };
    let mut rest = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    loop {
        rest = rest.trim_ascii_start();
        let skip = if rest.starts_with(b"<?") {
            after(rest, b"?>")?
        } else if rest.starts_with(b"<!--") {
            after(rest, b"-->")?
        } else if rest.starts_with(b"<!") {
            // A doctype may hold declarations of its own in brackets.
            match rest.iter().position(|byte| matches!(byte, b'[' | b'>'))? {
                at if rest[at] == b'[' => at + after(&rest[at..], b"]>")?,
                at => at + 1,
            }
        } else {
            break;
        };
        rest = &rest[skip..];
    }
    let rest = rest.strip_prefix(b"<")?;
    let name = rest
        .iter()
        .position(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/'))?;
    if rest[..name].rsplit(|byte| *byte == b':').next()? != b"svg" {
        return None;
    }
    let mut quote = None;
    for (at, byte) in rest.iter().enumerate().skip(name) {
        match (quote, byte) {
            (Some(open), _) if open == byte => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            (None, b'>') => return std::str::from_utf8(&rest[name..at]).ok(),
            (None, _) => {}
        }
    }
    None
}

/// One attribute's value out of a tag's attributes.
fn attribute<'a>(mut tag: &'a str, name: &str) -> Option<&'a str> {
    loop {
        let (key, rest) = tag.split_once('=')?;
        let rest = rest.trim_start();
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let (value, rest) = rest[1..].split_once(quote)?;
        if key.trim() == name {
            return Some(value);
        }
        tag = rest;
    }
}

/// An absolute SVG length in CSS pixels. A percentage and a length
/// relative to a font are `None`: neither is a size the file has on
/// its own.
fn svg_length(value: &str) -> Option<f32> {
    let value = value.trim();
    let number = value.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    let unit = &value[number.len()..];
    let per = [
        ("", 1.0),
        ("px", 1.0),
        ("pt", CSS_DPI / 72.0),
        ("pc", CSS_DPI / 6.0),
        ("in", CSS_DPI),
        ("cm", CSS_DPI / 2.54),
        ("mm", CSS_DPI / 25.4),
        ("q", CSS_DPI / 101.6),
    ]
    .into_iter()
    .find_map(|(name, per)| unit.eq_ignore_ascii_case(name).then_some(per))?;
    let pixels = number.parse::<f32>().ok()? * per;
    (pixels.is_finite() && pixels > 0.0).then_some(pixels)
}

/// The width and height of a `viewBox`.
fn view_box(value: &str) -> Option<(f32, f32)> {
    let mut numbers = value
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<f32>().ok());
    let (_, _, width, height) = (
        numbers.next()??,
        numbers.next()??,
        numbers.next()??,
        numbers.next()??,
    );
    (numbers.next().is_none() && width > 0.0 && height > 0.0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Attributes;

    /// A table loads what the host handed over, by its url, and
    /// nothing it refused.
    #[test]
    fn a_table_loads_the_bytes_a_url_was_registered_with() {
        let mut assets = Assets::none();
        let png = png_bytes(4, 2, None);
        assets.add("a.png", png.clone());
        assets.add("b.bmp", b"BM....".to_vec());
        assert_eq!(ImageLoader::load(&assets, "a.png"), Some(png));
        assert_eq!(ImageLoader::load(&assets, "b.bmp"), None);
        assert_eq!(ImageLoader::load(&assets, "c.png"), None);
    }

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

    /// An RGBA PNG whose alpha `covered` decides, pixel by pixel.
    fn rgba_png(width: u32, height: u32, covered: impl Fn(u32, u32) -> bool) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend([0x33, 0x44, 0x55, if covered(x, y) { 0xFF } else { 0 }]);
            }
        }
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("the header writes");
        writer.write_image_data(&pixels).expect("the pixels write");
        writer.finish().expect("the file closes");
        bytes
    }

    /// The leftmost and rightmost point a contour reaches between two
    /// heights, as a fraction of the image's width.
    fn span(contour: &Contour, top: f32, bottom: f32) -> Option<(f32, f32)> {
        let mut reach: Option<(f32, f32)> = None;
        for ring in &contour.rings {
            for point in ring {
                if point[1] < top || point[1] > bottom {
                    continue;
                }
                reach = Some(match reach {
                    None => (point[0], point[0]),
                    Some((left, right)) => (left.min(point[0]), right.max(point[0])),
                });
            }
        }
        reach
    }

    /// The alpha channel decides the contour: a shape that covers half
    /// of every row traces to a contour half the width of the image,
    /// and one that widens down the image traces to a contour that
    /// widens with it.
    #[test]
    fn an_alpha_channel_traces_to_the_shape_it_covers() {
        let half = trace(&rgba_png(64, 64, |x, _| x < 32)).expect("a PNG with alpha traces");
        assert_eq!(half.rings.len(), 1);
        assert_eq!(span(&half, 0.0, 1.0), Some((0.0, 0.5)));

        // A wedge: one pixel wide at the top, the whole width at the
        // bottom.
        let wedge = trace(&rgba_png(64, 64, |x, y| x <= y)).expect("a PNG with alpha traces");
        let (_, top) = span(&wedge, 0.0, 0.1).expect("the top of the wedge");
        let (_, bottom) = span(&wedge, 0.9, 1.0).expect("the foot of the wedge");
        assert!(top < 0.2, "the wedge opens narrow: {top}");
        assert!(bottom > 0.9, "and closes wide: {bottom}");
    }

    /// Clear space across the image ends one ring and opens another,
    /// so the prose can set through the gap.
    #[test]
    fn clear_space_across_an_image_splits_the_contour() {
        let split = trace(&rgba_png(64, 64, |_, y| !(24..40).contains(&y)))
            .expect("a PNG with alpha traces");
        assert_eq!(split.rings.len(), 2, "{:?}", split.rings.len());
        assert_eq!(span(&split, 0.4, 0.6), None, "nothing covers the gap");
    }

    /// An image with nothing in it traces to no rings at all, which is
    /// a contour the prose sets straight through.
    #[test]
    fn an_empty_alpha_channel_traces_to_nothing() {
        let empty = trace(&rgba_png(16, 16, |_, _| false)).expect("a PNG with alpha traces");
        assert!(empty.rings.is_empty());
    }

    /// A format that carries no alpha traces to nothing, and its box
    /// is what the prose keeps clear of. So do bytes that do not
    /// decode.
    #[test]
    fn a_format_without_alpha_traces_to_nothing() {
        assert!(trace(&jpeg_bytes(64, 64, None)).is_none());
        assert!(trace(&png_bytes(64, 64, None)).is_none(), "a bare header");
        assert!(trace(b"not an image").is_none());
        assert!(trace(b"").is_none());
    }

    /// The same picture in two formats traces to the same contour:
    /// the outline is the image's shape, not the file's.
    #[test]
    fn one_shape_in_two_formats_traces_the_same() {
        let png = trace(include_bytes!("../../../fixtures/images/fleuron.png"))
            .expect("the ornament traces");
        let webp = trace(include_bytes!("../../../fixtures/images/fleuron.webp"))
            .expect("the ornament traces");
        assert_eq!(png.rings.len(), 1, "the ornament is one shape");
        for (top, bottom) in [(0.0, 0.25), (0.25, 0.5), (0.5, 0.75), (0.75, 1.0)] {
            let (one, two) = (span(&png, top, bottom), span(&webp, top, bottom));
            let (one, two) = (one.expect("the PNG covers it"), two.expect("the WebP does"));
            assert!(
                (one.0 - two.0).abs() < 0.02 && (one.1 - two.1).abs() < 0.02,
                "between {top} and {bottom}: {one:?} against {two:?}",
            );
        }
        // The ornament is set on a clear ground, so its contour is
        // narrower than its box.
        let widest = (0..8)
            .filter_map(|band| span(&png, band as f32 / 8.0, (band as f32 + 1.0) / 8.0))
            .fold(0.0f32, |widest, (left, right)| widest.max(right - left));
        assert!(widest < 0.95, "the contour is the box: {widest}");
    }

    /// A GIF's transparent palette entry is an alpha channel like any
    /// other.
    #[test]
    fn a_transparent_palette_entry_traces_like_an_alpha_channel() {
        let (width, height) = (32u16, 32u16);
        let indices: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).map(move |x| u8::from(x >= y)))
            .collect();
        let mut bytes = Vec::new();
        {
            let mut encoder =
                gif::Encoder::new(&mut bytes, width, height, &[0, 0, 0, 0x33, 0x44, 0x55])
                    .expect("the header writes");
            let frame = gif::Frame {
                width,
                height,
                buffer: std::borrow::Cow::Borrowed(&indices),
                transparent: Some(0),
                ..gif::Frame::default()
            };
            encoder.write_frame(&frame).expect("the frame writes");
        }
        let traced = trace(&bytes).expect("a GIF with a transparent entry traces");
        assert_eq!(traced.rings.len(), 1);
        let (left, _) = span(&traced, 0.0, 0.1).expect("the top of the wedge");
        let (foot, _) = span(&traced, 0.9, 1.0).expect("the foot of it");
        assert!(left < 0.1, "the wedge opens at the left edge: {left}");
        assert!(foot > 0.8, "and closes at the right: {foot}");
    }

    /// Two runs over one image trace the same bytes to the same
    /// contour.
    #[test]
    fn tracing_is_deterministic() {
        let bytes = rgba_png(48, 32, |x, y| (x + y) % 17 < 9);
        assert_eq!(trace(&bytes), trace(&bytes));
    }

    /// Every format the probe recognises, read back at its declared size.
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

    /// Part: the probe reads an SVG root for `width`, `height` and
    /// `viewBox`, and gives an intrinsic size and a ratio. An XML
    /// declaration, a doctype and a comment before the root are
    /// stepped over.
    #[test]
    fn an_svg_root_gives_up_its_size() {
        let plain = probe(br#"<svg xmlns="http://www.w3.org/2000/svg" width="192" height="96"/>"#)
            .expect("an SVG with a size probes");
        assert_eq!((plain.width, plain.height), (192, 96));
        assert_eq!(plain.size(), (144.0, 72.0));
        assert!(plain.sized);

        let prefaced = probe(
            b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<!-- a > rule -->\n\
              <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"svg11.dtd\" [<!ENTITY a \"b\">]>\n\
              <svg:svg title='a > b' width='1in' height=\"36pt\" viewBox=\"0 0 10 10\">",
        )
        .expect("the root is found after its preamble");
        assert_eq!(prefaced.size(), (72.0, 36.0));

        // A size no whole number of pixels holds is still exact.
        let metric = probe(br#"<svg width="10mm" height="2.5cm">"#).expect("metric units probe");
        let (width, height) = metric.size();
        assert!((width - 28.346).abs() < 0.001, "{width}");
        assert!((height - 70.866).abs() < 0.001, "{height}");
    }

    /// Part: a percentage size or no size falls back to the `viewBox`
    /// ratio, and one side given takes the other from it.
    #[test]
    fn an_svg_without_a_size_falls_back_to_its_view_box() {
        for root in [
            r#"<svg viewBox="0 0 300 100">"#,
            r#"<svg width="100%" height="100%" viewBox="0,0,300,100">"#,
            r#"<svg width="20em" viewBox="-5 -5 300 100">"#,
        ] {
            let ratio = probe(root.as_bytes()).unwrap_or_else(|| panic!("{root} probes"));
            assert!(!ratio.sized, "{root} has no size of its own");
            assert_eq!(ratio.size(), (225.0, 75.0), "{root}");
            assert_eq!(ratio.size_within(360.0), (360.0, 120.0), "{root}");
        }
        let wide = probe(br#"<svg width="150" viewBox="0 0 300 100">"#).unwrap();
        assert!(wide.sized);
        assert_eq!((wide.width, wide.height), (150, 50));
        let tall = probe(br#"<svg height="50" viewBox="0 0 300 100">"#).unwrap();
        assert_eq!((tall.width, tall.height), (150, 50));
        assert_eq!(
            tall.size_within(360.0),
            tall.size(),
            "a sized image keeps its size"
        );
    }

    /// Part: an SVG with no size and no `viewBox` warns and is
    /// skipped, the way an unreadable file is. The warning is raised
    /// once however often the url is offered.
    #[test]
    fn an_svg_with_no_size_and_no_view_box_is_refused_once() {
        for root in [
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0\"/></svg>",
            "<svg width=\"100%\" height=\"50%\">",
            "<svg width=\"120\">",
            "<svg viewBox=\"0 0 0 10\">",
            "<svg viewBox=\"0 0 10\">",
            "<html><svg width=\"1\" height=\"1\"/></html>",
            "<svg width=\"10\" height=\"10\"",
        ] {
            assert!(probe(root.as_bytes()).is_none(), "{root} probed");
        }
        let mut assets = Assets::none();
        let bare = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec();
        assert_eq!(assets.add("rule.svg", bare.clone()), Added::Refused);
        assert_eq!(assets.add("rule.svg", bare), Added::Refused);
        assert_eq!(assets.warnings().len(), 1);
        assert_eq!(
            assets.warnings()[0].message,
            "Image rule.svg has no size in it. The image is skipped.",
        );
        assert!(assets.assets().is_empty());
    }

    /// An SVG root cut off anywhere is `None` rather than a panic.
    #[test]
    fn a_truncated_svg_probes_to_nothing() {
        let svg = "<?xml version=\"1.0\"?><!-- c --><!DOCTYPE svg [ ]>\
                   <svg width=\"10\" height='10' viewBox=\"0 0 1 1\">";
        for cut in 0..svg.len() {
            assert!(probe(&svg.as_bytes()[..cut]).is_none(), "cut at {cut}");
        }
        assert!(probe(svg.as_bytes()).is_some());
    }

    /// An SVG has no alpha channel to trace, so it traces to nothing
    /// and its box is what the text goes around.
    #[test]
    fn an_svg_traces_to_nothing() {
        let svg = br#"<svg width="64" height="64"><circle cx="32" cy="32" r="16"/></svg>"#;
        assert!(is_svg(svg));
        assert!(trace(svg).is_none());
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

    /// A host that pushes gets the same table a loader would have
    /// filled: the header sizes the image, the file is kept for the
    /// painter, and bytes no probe recognises are one complaint and no
    /// asset.
    #[test]
    fn pushed_images_are_probed_and_kept() {
        let mut assets = Assets::none();
        let png = png_bytes(96, 48, None);
        let a = assets.add("a.png", png.clone());
        assert!(matches!(
            a,
            Added::Replaced {
                index: 0,
                previous: None,
                ..
            }
        ));
        let b = assets.add("b.jpg", jpeg_bytes(200, 100, None));
        assert!(matches!(
            b,
            Added::Replaced {
                index: 1,
                previous: None,
                ..
            }
        ));
        // The same url and the same bytes twice is the same asset,
        // not a second copy, and costs nothing.
        assert_eq!(assets.add("a.png", png.clone()), Added::Unchanged(0));
        assert_eq!(assets.bytes(0), Some(png.as_slice()));
        assert_eq!(assets.lookup("a.png").map(|(index, _)| index), Some(0));

        assert_eq!(
            assets.add("c.txt", b"not an image".to_vec()),
            Added::Refused
        );
        assert!(assets.probed("c.txt"), "a refusal counts as probed");
        assert!(!assets.probed("d.png"), "a url nobody offered does not");
        assert_eq!(assets.warnings().len(), 1);
        assert!(assets.warnings()[0].message.contains("c.txt"));
        // Offered twice, complained about once.
        assert_eq!(assets.add("c.txt", b"still not".to_vec()), Added::Refused);
        assert_eq!(assets.warnings().len(), 1);
    }

    /// The same url registered again with different bytes replaces
    /// them in place, keeping the index `DrawItem::Image.asset`
    /// already names, and says what the size was before so a caller
    /// can tell whether the box an image takes moved.
    #[test]
    fn different_bytes_at_the_same_url_replace_it_in_place() {
        let mut assets = Assets::none();
        assets.add("a.png", png_bytes(96, 48, None));

        let same_size = assets.add("a.png", png_bytes(96, 48, Some(150)));
        let Added::Replaced {
            index,
            previous,
            current,
        } = same_size
        else {
            panic!("different bytes at a registered url did not replace it: {same_size:?}");
        };
        assert_eq!(index, 0, "the index moved");
        assert_eq!(previous.map(|i| (i.width, i.height)), Some((96, 48)));
        assert_eq!((current.width, current.height), (96, 48));
        assert_eq!(
            assets.bytes(0),
            Some(png_bytes(96, 48, Some(150)).as_slice())
        );

        let resized = assets.add("a.png", png_bytes(200, 100, None));
        let Added::Replaced {
            index,
            previous,
            current,
        } = resized
        else {
            panic!("a resize did not replace the asset: {resized:?}");
        };
        assert_eq!(index, 0);
        assert_eq!(previous.map(|i| (i.width, i.height)), Some((96, 48)));
        assert_eq!((current.width, current.height), (200, 100));
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
            attributes: Attributes::default(),
            position: None,
            span: None,
        };
        let mut book = Book {
            metadata: Default::default(),
            sections: vec![crate::content::Section {
                attributes: Default::default(),
                blocks: vec![
                    image("a.png"),
                    Block::Blockquote {
                        id: crate::content::NodeId::UNASSIGNED,
                        blocks: vec![image("b.jpg")],
                        attributes: Attributes::default(),
                        position: None,
                        span: None,
                    },
                    image("a.png"),
                    image("missing.png"),
                ],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        let styles = crate::style::defaults(
            &book,
            crate::fonts::bundled_registry()
                .as_ref()
                .expect("the bundled face parses"),
        );
        let assets = Assets::probe(&book, &styles, &Two);
        assert_eq!(assets.assets().len(), 2, "a.png was probed twice");
        assert_eq!(assets.lookup("a.png").map(|(index, _)| index), Some(0));
        assert_eq!(assets.lookup("b.jpg").map(|(index, _)| index), Some(1));
        assert_eq!(assets.lookup("missing.png"), None);
        assert_eq!(assets.warnings().len(), 1);
        assert!(assets.warnings()[0].message.contains("missing.png"));
    }

    /// A book of one paragraph, with no image in it.
    fn plain_book() -> Book {
        let mut book = Book {
            metadata: Default::default(),
            sections: vec![crate::content::Section {
                attributes: Default::default(),
                blocks: vec![Block::Paragraph {
                    id: crate::content::NodeId::UNASSIGNED,
                    inlines: vec![crate::content::Inline::Text {
                        id: crate::content::NodeId::UNASSIGNED,
                        value: "Nothing is placed here.".into(),
                        attributes: Attributes::default(),
                        position: None,
                        span: None,
                    }],
                    attributes: Attributes::default(),
                    position: None,
                    span: None,
                }],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        book
    }

    /// One book under one sheet.
    fn styled(book: &Book, css: &str) -> StyleTree {
        let registry = crate::fonts::bundled_registry().expect("the bundled face parses");
        let styles = crate::style::Stylesheets::parse(&[crate::style::Source::author(
            "background.css",
            css,
        )])
        .compile(book, &registry);
        assert!(
            styles.warnings().is_empty(),
            "the sheet is in the subset: {:?}",
            styles.warnings(),
        );
        styles
    }

    /// A loader that counts what it was asked for.
    struct Counting(std::cell::RefCell<Vec<String>>);

    impl ImageLoader for Counting {
        fn load(&self, url: &str) -> Option<Vec<u8>> {
            self.0.borrow_mut().push(url.to_string());
            (url != "missing.png").then(|| png_bytes(96, 48, None))
        }
    }

    /// A url the sheet names reaches the asset table, from a block
    /// rule and from `@page` alike, and the manuscript's own images
    /// are indexed first.
    #[test]
    fn a_url_the_sheet_names_reaches_the_asset_table() {
        let book = plain_book();
        let styles = styled(
            &book,
            "@page { background-image: url(\"scan.png\") }\n\
             p { background-image: url(tint.png) }",
        );
        let loader = Counting(std::cell::RefCell::new(Vec::new()));
        let assets = Assets::probe(&book, &styles, &loader);

        let mut named: Vec<&str> = assets.assets().iter().map(|a| a.url.as_str()).collect();
        named.sort_unstable();
        assert_eq!(named, ["scan.png", "tint.png"]);
        assert!(assets.warnings().is_empty(), "{:?}", assets.warnings());
    }

    /// A book whose sheet names no background image offers the loader
    /// nothing of its own: the cascade is read, and nothing is probed
    /// for it.
    #[test]
    fn a_sheet_that_names_no_background_probes_nothing() {
        let book = plain_book();
        let styles = styled(&book, "p { background-color: #f4f1ea }");
        let loader = Counting(std::cell::RefCell::new(Vec::new()));
        let assets = Assets::probe(&book, &styles, &loader);

        assert!(
            loader.0.borrow().is_empty(),
            "the loader was asked for {:?}",
            loader.0.borrow(),
        );
        assert!(assets.assets().is_empty());
    }

    /// A url nothing resolves warns, and the warning names the line
    /// and column the sheet wrote it at.
    #[test]
    fn a_url_nothing_resolves_names_where_it_was_written() {
        let book = plain_book();
        let styles = styled(&book, "p {\n  background-image: url(missing.png);\n}");
        let loader = Counting(std::cell::RefCell::new(Vec::new()));
        let assets = Assets::probe(&book, &styles, &loader);

        assert!(assets.assets().is_empty());
        assert_eq!(assets.warnings().len(), 1);
        assert!(assets.warnings()[0].message.contains("missing.png"));
        assert_eq!(
            assets.warnings()[0].origin.as_deref(),
            Some("background.css:2:3"),
        );
    }

    /// An asset probed through a loader hashes the same as one pushed
    /// through `add`: registering it again, with the bytes it already
    /// has or with different ones, does not panic and answers the
    /// same way either door would have.
    #[test]
    fn probed_and_pushed_assets_share_one_hash_table() {
        struct One;
        impl ImageLoader for One {
            fn load(&self, url: &str) -> Option<Vec<u8>> {
                (url == "a.png").then(|| png_bytes(96, 48, None))
            }
        }

        let image = |url: &str| Block::Image {
            id: crate::content::NodeId::UNASSIGNED,
            url: url.into(),
            alt: String::new(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        };
        let mut book = Book {
            metadata: Default::default(),
            sections: vec![crate::content::Section {
                attributes: Default::default(),
                blocks: vec![image("a.png")],
                ..Default::default()
            }],
        };
        book.assign_node_ids();
        let styles = crate::style::defaults(
            &book,
            crate::fonts::bundled_registry()
                .as_ref()
                .expect("the bundled face parses"),
        );
        let mut assets = Assets::probe(&book, &styles, &One);

        assert_eq!(
            assets.add("a.png", png_bytes(96, 48, None)),
            Added::Unchanged(0),
            "the same bytes probed and pushed hash the same"
        );
        let resized = assets.add("a.png", png_bytes(200, 100, None));
        assert!(
            matches!(resized, Added::Replaced { index: 0, .. }),
            "different bytes replaced the probed asset in place: {resized:?}"
        );
    }
}
