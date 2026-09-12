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

    /// What the backdrop paints over the box at `(x, y)`, in `layer`,
    /// tint first and image over it.
    pub(super) fn items(&self, x: f32, y: f32, w: f32, h: f32, layer: i32) -> Vec<DrawItem> {
        let mut items = Vec::new();
        if w <= 0.0 || h <= 0.0 {
            return items;
        }
        if let Some(color) = self.color {
            items.push(DrawItem::Rect {
                x,
                y,
                w,
                h,
                color,
                layer,
            });
        }
        if let Some(tile) = &self.image {
            items.extend(tile.item(x, y, w, h, layer));
        }
        items
    }
}

impl Tile {
    /// The one item this image paints over a box, or `None` where it
    /// is drawn at no size at all.
    fn item(&self, x: f32, y: f32, w: f32, h: f32, layer: i32) -> Option<DrawItem> {
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
            layer,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{BackgroundPosition, Url};

    /// An image 80 by 40 points, at the CSS resolution.
    fn intrinsic() -> Intrinsic {
        Intrinsic {
            width: 80,
            height: 40,
            dpi_x: 72.0,
            dpi_y: 72.0,
        }
    }

    fn backdrop(size: BackgroundSize, position: BackgroundPosition) -> Backdrop {
        Backdrop::of(
            &Background {
                image: Some(Url::new("plate.png")),
                size,
                position,
                ..Background::NONE
            },
            Some((0, intrinsic())),
        )
    }

    /// The tile one backdrop draws over a box 200 by 100 points at the
    /// page's own corner: `(left, top, width, height)`.
    fn tile(backdrop: &Backdrop) -> (f32, f32, f32, f32) {
        match backdrop.items(0.0, 0.0, 200.0, 100.0, 0).pop() {
            Some(DrawItem::Background {
                tile_x,
                tile_y,
                tile_w,
                tile_h,
                ..
            }) => (tile_x, tile_y, tile_w, tile_h),
            other => panic!("the backdrop painted {other:?}"),
        }
    }

    /// `auto` draws the image at the size its header asks for.
    #[test]
    fn an_unsized_image_is_drawn_at_its_own_size() {
        let backdrop = backdrop(BackgroundSize::Auto, BackgroundPosition::ORIGIN);
        assert_eq!(tile(&backdrop), (0.0, 0.0, 80.0, 40.0));
    }

    /// `cover` is the smallest size that covers the box, so the image
    /// reaches past the box on the axis it does not fit.
    #[test]
    fn cover_fills_the_box_and_leaves_the_rest_to_the_crop() {
        let backdrop = backdrop(BackgroundSize::Cover, BackgroundPosition::ORIGIN);
        let (_, _, width, height) = tile(&backdrop);
        assert_eq!((width, height), (200.0, 100.0));
    }

    /// A box of another ratio still covers, and the overflow is on
    /// the axis the scale ran past.
    #[test]
    fn cover_keeps_the_images_own_ratio() {
        let backdrop = backdrop(BackgroundSize::Cover, BackgroundPosition::ORIGIN);
        let Some(DrawItem::Background {
            tile_w,
            tile_h,
            w,
            h,
            ..
        }) = backdrop.items(0.0, 0.0, 200.0, 200.0, 0).pop()
        else {
            panic!("the backdrop painted nothing");
        };
        assert_eq!((tile_w, tile_h), (400.0, 200.0));
        assert!(tile_w > w && tile_h <= h, "cover left the box uncovered");
    }

    /// `contain` is the largest size that fits, so the box keeps the
    /// remainder on one axis.
    #[test]
    fn contain_fits_the_box_and_leaves_the_remainder() {
        let backdrop = backdrop(BackgroundSize::Contain, BackgroundPosition::ORIGIN);
        let (_, _, width, height) = tile(&backdrop);
        assert_eq!((width, height), (200.0, 100.0));

        let Some(DrawItem::Background { tile_w, tile_h, .. }) =
            backdrop.items(0.0, 0.0, 200.0, 200.0, 0).pop()
        else {
            panic!("the backdrop painted nothing");
        };
        assert_eq!((tile_w, tile_h), (200.0, 100.0));
    }

    /// One length sizes the image across the box, and its own ratio
    /// sizes it down the box.
    #[test]
    fn one_length_sizes_the_other_axis_by_the_ratio() {
        let backdrop = backdrop(
            BackgroundSize::Fixed {
                width: Some(Coord::Points(40.0)),
                height: None,
            },
            BackgroundPosition::ORIGIN,
        );
        assert_eq!(tile(&backdrop), (0.0, 0.0, 40.0, 20.0));
    }

    /// A percentage size measures against the box the image is
    /// behind.
    #[test]
    fn a_percentage_size_measures_against_the_box() {
        let backdrop = backdrop(
            BackgroundSize::Fixed {
                width: Some(Coord::Percent(50.0)),
                height: Some(Coord::Percent(100.0)),
            },
            BackgroundPosition::ORIGIN,
        );
        assert_eq!(tile(&backdrop), (0.0, 0.0, 100.0, 100.0));
    }

    /// A percentage position aligns that fraction of the image with
    /// the same fraction of the box, which is what centres it.
    #[test]
    fn a_percentage_position_aligns_the_image_with_the_box() {
        let centred = backdrop(
            BackgroundSize::Auto,
            BackgroundPosition {
                x: Coord::Percent(50.0),
                y: Coord::Percent(50.0),
            },
        );
        assert_eq!(tile(&centred), (60.0, 30.0, 80.0, 40.0));

        let corner = backdrop(
            BackgroundSize::Auto,
            BackgroundPosition {
                x: Coord::Percent(100.0),
                y: Coord::Percent(100.0),
            },
        );
        assert_eq!(tile(&corner), (120.0, 60.0, 80.0, 40.0));
    }

    /// A length position is an offset from the box's own corner.
    #[test]
    fn a_length_position_is_an_offset_from_the_corner() {
        let backdrop = backdrop(
            BackgroundSize::Auto,
            BackgroundPosition {
                x: Coord::Points(12.0),
                y: Coord::Points(18.0),
            },
        );
        assert_eq!(tile(&backdrop), (12.0, 18.0, 80.0, 40.0));
    }

    /// The tint is painted first and the image over it, so a block
    /// that names both shows the colour wherever the image does not
    /// reach.
    #[test]
    fn a_tinted_and_imaged_box_paints_the_colour_under_the_image() {
        let backdrop = Backdrop::of(
            &Background {
                color: Some(Color::rgb(0xf4, 0xf1, 0xea)),
                image: Some(Url::new("plate.png")),
                ..Background::NONE
            },
            Some((0, intrinsic())),
        );
        let items = backdrop.items(0.0, 0.0, 200.0, 100.0, 0);
        assert!(
            matches!(items[0], DrawItem::Rect { .. }),
            "the tint is not painted first: {items:?}",
        );
        assert!(matches!(items[1], DrawItem::Background { .. }));
    }

    /// A url the asset table does not answer for leaves the tint and
    /// paints no image.
    #[test]
    fn an_unresolved_url_leaves_the_box_painting_its_tint() {
        let backdrop = Backdrop::of(
            &Background {
                color: Some(Color::rgb(0xf4, 0xf1, 0xea)),
                image: Some(Url::new("missing.png")),
                ..Background::NONE
            },
            None,
        );
        let items = backdrop.items(0.0, 0.0, 200.0, 100.0, 0);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], DrawItem::Rect { .. }));
    }
}
