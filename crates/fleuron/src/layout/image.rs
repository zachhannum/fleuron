//! The size an image is set at.

use crate::style::ComputedStyle;

use super::Paginator;

impl Paginator<'_> {
    /// Sizes the image at `url` with `size`, and warns where the page
    /// made it smaller than the image asked for.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn image_size(
        &self,
        style: &ComputedStyle,
        url: &str,
        intrinsic: (f32, f32),
        within: (f32, f32),
        measure: f32,
        room: f32,
        origin: Option<String>,
    ) -> ImageSize {
        let size = size(style, intrinsic, within, measure, room);
        if size.tall {
            self.warn(
                format!("Image {url} is taller than the page. It is scaled to fit."),
                origin,
            );
        } else if size.wide {
            self.warn(
                format!("Image {url} is wider than the content box. It is scaled to fit."),
                origin,
            );
        }
        size
    }
}

/// The size one image is set at, and whether the page had to
/// make it smaller than the size it asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ImageSize {
    pub(super) width: f32,
    pub(super) height: f32,
    /// A `width` or a `height` the sheet asked for came out wider than
    /// the content box.
    pub(super) wide: bool,
    /// The image came out taller than the content box, once it fit
    /// the width.
    pub(super) tall: bool,
}

/// Sizes an image as CSS 2.1 §10.4 sizes a replaced element.
///
/// `within` is the width a percentage on the width axis resolves
/// against, and `tall_within` the height one on the height axis does.
/// `measure` and `room` are the content box that has to hold the
/// image. They are the page's limit rather than a default, so they
/// apply over any size the sheet asked for.
pub(super) fn size(
    style: &ComputedStyle,
    intrinsic: (f32, f32),
    (within, tall_within): (f32, f32),
    measure: f32,
    room: f32,
) -> ImageSize {
    let (own_width, own_height) = intrinsic;
    let ratio = (own_width > 0.0 && own_height > 0.0).then(|| own_width / own_height);
    let asked = (
        style.width.resolve(within),
        style.height.resolve(tall_within),
    );
    let (mut width, mut height) = match asked {
        (Some(width), Some(height)) => (width, height),
        (Some(width), None) => (width, ratio.map_or(own_height, |ratio| width / ratio)),
        (None, Some(height)) => (ratio.map_or(own_width, |ratio| height * ratio), height),
        (None, None) => intrinsic,
    };
    let most = (
        style.max_width.resolve(within).unwrap_or(f32::INFINITY),
        style
            .max_height
            .resolve(tall_within)
            .unwrap_or(f32::INFINITY),
    );
    // With both sides given, the ratio is the author's, so each side
    // meets its own ceiling.
    if let (Some(_), Some(_)) = asked {
        width = width.min(most.0);
        height = height.min(most.1);
    } else {
        (width, height) = fit((width, height), most.0, most.1);
    }
    let asked = asked.0.is_some() || asked.1.is_some();
    let wide = asked && width > measure;
    let (width, height) = fit((width, height), measure, f32::INFINITY);
    let tall = height > room;
    let (width, height) = fit((width, height), measure, room);
    ImageSize {
        width,
        height,
        wide,
        tall,
    }
}

/// An image's size inside a box that has to hold it: its own, scaled
/// down in proportion where either side does not fit.
pub(super) fn fit((width, height): (f32, f32), available: f32, room: f32) -> (f32, f32) {
    let scale = |value: f32, from: f32, to: f32| {
        if from > 0.0 { value * to / from } else { value }
    };
    let (mut width, mut height) = (width, height);
    if width > available {
        height = scale(height, width, available);
        width = available;
    }
    if height > room {
        width = scale(width, height, room);
        height = room;
    }
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Width;

    /// An image 80 by 40 points, in a content box 300 wide and 600 tall.
    fn sized(set: impl FnOnce(&mut ComputedStyle)) -> ImageSize {
        let mut style = ComputedStyle::initial();
        set(&mut style);
        size(&style, (80.0, 40.0), (300.0, 600.0), 300.0, 600.0)
    }

    fn pair(size: ImageSize) -> (f32, f32) {
        (size.width, size.height)
    }

    /// Part: with no size of its own, an image keeps its intrinsic
    /// size.
    #[test]
    fn an_unsized_image_keeps_its_intrinsic_size() {
        assert_eq!(pair(sized(|_| {})), (80.0, 40.0));
    }

    /// Part: one side given, the other follows the intrinsic ratio.
    #[test]
    fn one_side_given_the_other_follows_the_ratio() {
        assert_eq!(
            pair(sized(|style| style.width = Width::Points(200.0))),
            (200.0, 100.0)
        );
        assert_eq!(
            pair(sized(|style| style.height = Width::Points(20.0))),
            (40.0, 20.0)
        );
    }

    /// Part: both sides given, the image takes both and drops its ratio.
    #[test]
    fn both_sides_given_the_ratio_is_the_authors() {
        assert_eq!(
            pair(sized(|style| {
                style.width = Width::Points(100.0);
                style.height = Width::Points(100.0);
            })),
            (100.0, 100.0)
        );
    }

    /// Part: a percentage width resolves against the width it is given
    /// and a percentage height against the height.
    #[test]
    fn percentages_resolve_against_their_own_axis() {
        assert_eq!(
            pair(sized(|style| style.width = Width::Percent(50.0))),
            (150.0, 75.0)
        );
        assert_eq!(
            pair(sized(|style| style.height = Width::Percent(10.0))),
            (120.0, 60.0)
        );
    }

    /// Part: `max-width` and `max-height` lower the size in proportion,
    /// and do nothing to an image already inside them.
    #[test]
    fn a_ceiling_lowers_the_size_in_proportion() {
        assert_eq!(
            pair(sized(|style| style.max_width = Width::Points(40.0))),
            (40.0, 20.0)
        );
        assert_eq!(
            pair(sized(|style| {
                style.width = Width::Points(200.0);
                style.max_height = Width::Percent(10.0);
            })),
            (120.0, 60.0)
        );
        assert_eq!(
            pair(sized(|style| style.max_width = Width::Points(400.0))),
            (80.0, 40.0)
        );
    }

    /// Part: with both sides given, each side meets its own ceiling.
    #[test]
    fn with_both_sides_given_each_side_meets_its_own_ceiling() {
        assert_eq!(
            pair(sized(|style| {
                style.width = Width::Points(100.0);
                style.height = Width::Points(100.0);
                style.max_width = Width::Points(50.0);
            })),
            (50.0, 100.0)
        );
    }

    /// Part: fit-to-measure and fit-to-page apply over an explicit size,
    /// keeping the size's own ratio, and say that they did.
    #[test]
    fn the_content_box_limits_an_explicit_size() {
        let wide = sized(|style| style.width = Width::Points(600.0));
        assert_eq!(pair(wide), (300.0, 150.0));
        assert!(wide.wide && !wide.tall);

        let tall = sized(|style| style.height = Width::Points(1200.0));
        assert_eq!(pair(tall), (300.0, 150.0));
        assert!(tall.wide && !tall.tall);

        let narrow = sized(|style| {
            style.width = Width::Points(200.0);
            style.height = Width::Points(900.0);
        });
        assert!((narrow.height - 600.0).abs() < 1e-3);
        assert!((narrow.width - 200.0 * 600.0 / 900.0).abs() < 1e-3);
        assert!(narrow.tall && !narrow.wide);
    }

    /// An image wider than the measure only because its file is wide is
    /// scaled without being flagged: nothing in the sheet asked for it.
    #[test]
    fn an_intrinsic_size_wider_than_the_measure_is_not_flagged() {
        let style = ComputedStyle::initial();
        let wide = size(&style, (800.0, 400.0), (300.0, 600.0), 300.0, 600.0);
        assert_eq!(pair(wide), (300.0, 150.0));
        assert!(!wide.wide && !wide.tall);
    }
}
