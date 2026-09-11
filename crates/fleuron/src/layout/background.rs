//! What goes behind a box: the tint, and the image over it.
//!
//! A page and a block paint the same way, so both resolve to a
//! [`Backdrop`] and both hand it the box they cover. The box is
//! settled late — a page when the page closes, a block when the
//! paginator knows which of its fragments landed where — so the
//! backdrop keeps what the cascade said and works the tile out once
//! the box is known.

use crate::images::Intrinsic;
use crate::pages::DrawItem;
use crate::style::{Background, BackgroundRepeat, BackgroundSize, Color, Coord};

/// What one box paints behind its content, with the image's own size
/// already read from its header.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Backdrop {
    /// The tint, painted first.
    pub(super) color: Option<Color>,
    /// The image over it, where the sheet named one the asset table
    /// answers for.
    pub(super) image: Option<Tile>,
}

/// The image behind a box, and what the cascade said about drawing
/// it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Tile {
    /// Index into the asset table.
    asset: u32,
    /// The size the image's own header asks for, in points.
    intrinsic: (f32, f32),
    repeat: BackgroundRepeat,
    size: BackgroundSize,
    position: Coords,
}

/// Where the image sits, as the cascade computed it.
type Coords = (Coord, Coord);

impl Backdrop {
    /// The backdrop one style asks for, over an asset the table
    /// answers for. `found` is what the url resolved to, and `None`
    /// leaves the box painting its tint alone.
    pub(super) fn of(background: &Background, found: Option<(u32, Intrinsic)>) -> Backdrop {
        Backdrop {
            color: background.color,
            image: background
                .image
                .as_ref()
                .and(found)
                .map(|(asset, intrinsic)| Tile {
                    asset,
                    intrinsic: intrinsic.size(),
                    repeat: background.repeat,
                    size: background.size,
                    position: (background.position.x, background.position.y),
                }),
        }
    }

    /// Whether this backdrop paints anything at all.
    pub(super) fn paints(&self) -> bool {
        self.color.is_some() || self.image.is_some()
    }

    /// What the backdrop paints over the box at `(x, y)`, tint first
    /// and image over it.
    pub(super) fn items(&self, x: f32, y: f32, w: f32, h: f32) -> Vec<DrawItem> {
        let mut items = Vec::new();
        if w <= 0.0 || h <= 0.0 {
            return items;
        }
        if let Some(color) = self.color {
            items.push(DrawItem::Rect { x, y, w, h, color });
        }
        if let Some(tile) = &self.image {
            items.extend(tile.item(x, y, w, h));
        }
        items
    }
}

impl Tile {
    /// The one item this image paints over a box, or `None` where it
    /// is drawn at no size at all.
    fn item(&self, x: f32, y: f32, w: f32, h: f32) -> Option<DrawItem> {
        let (tile_w, tile_h) = self.drawn(w, h);
        if tile_w <= 0.0 || tile_h <= 0.0 {
            return None;
        }
        Some(DrawItem::Background {
            x,
            y,
            w,
            h,
            tile_x: x + offset(self.position.0, w, tile_w),
            tile_y: y + offset(self.position.1, h, tile_h),
            tile_w,
            tile_h,
            repeat: self.repeat == BackgroundRepeat::Repeat,
            asset: self.asset,
        })
    }

    /// The size one copy of the image is drawn at, over a box `w` by
    /// `h`. An axis the sheet left `auto` follows the image's own
    /// ratio, and an image with no ratio to follow is drawn at the
    /// size its header asks for.
    fn drawn(&self, w: f32, h: f32) -> (f32, f32) {
        let (image_w, image_h) = self.intrinsic;
        match self.size {
            BackgroundSize::Auto => (image_w, image_h),
            BackgroundSize::Cover | BackgroundSize::Contain => {
                if image_w <= 0.0 || image_h <= 0.0 {
                    return (image_w, image_h);
                }
                let (across, down) = (w / image_w, h / image_h);
                let scale = if self.size == BackgroundSize::Cover {
                    across.max(down)
                } else {
                    across.min(down)
                };
                (image_w * scale, image_h * scale)
            }
            BackgroundSize::Fixed { width, height } => {
                let ratio = (image_w > 0.0 && image_h > 0.0).then(|| image_w / image_h);
                match (
                    width.map(|width| width.to_points(w).max(0.0)),
                    height.map(|height| height.to_points(h).max(0.0)),
                ) {
                    (Some(width), Some(height)) => (width, height),
                    (Some(width), None) => (width, ratio.map_or(image_h, |ratio| width / ratio)),
                    (None, Some(height)) => (ratio.map_or(image_w, |ratio| height * ratio), height),
                    (None, None) => (image_w, image_h),
                }
            }
        }
    }
}

/// Where one axis of the image starts, from the box's own edge.
///
/// A percentage aligns that fraction of the image with the same
/// fraction of the box, which is what puts `50%` in the middle
/// whatever size either of them is. A length is an offset from the
/// edge.
fn offset(position: Coord, box_extent: f32, tile_extent: f32) -> f32 {
    match position {
        Coord::Points(points) => points,
        Coord::Percent(percent) => percent / 100.0 * (box_extent - tile_extent),
    }
}
