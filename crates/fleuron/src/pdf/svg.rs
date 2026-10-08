//! An SVG as PDF content: the tree usvg parses, drawn as the paths,
//! paints, clips and masks it holds, so the art stays vector in the
//! export.
//!
//! Two things an SVG can ask for have no vector form here. Text that
//! was not converted to outlines is not drawn, since the file names a
//! face and carries none. A filter effect is a raster operation, so a
//! group that has one is drawn without it.

use std::sync::Arc;

use krilla::blend::BlendMode;
use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Rect, Size, Transform};
use krilla::image::Image;
use krilla::mask::{Mask, MaskType};
use krilla::num::NormalizedF32;
use krilla::paint::{
    Fill, FillRule, LineCap, LineJoin, LinearGradient, Paint, Pattern, RadialGradient,
    SpreadMethod, Stop, Stroke, StrokeDash,
};
use krilla::surface::Surface;
use usvg::tiny_skia_path::PathSegment;

/// One SVG, parsed once for the whole export.
pub(super) struct Vector(usvg::Tree);

impl Vector {
    /// Parses a file. `None` where it is not an SVG usvg can read.
    pub(super) fn parse(bytes: &[u8]) -> Option<Vector> {
        let mut options = usvg::Options::default();
        // The engine opens no file. An image inside the SVG is drawn
        // where the SVG carries its bytes, and not where it names a
        // path.
        options.image_href_resolver.resolve_string = Box::new(|_, _| None);
        usvg::Tree::from_data(bytes, &options).ok().map(Vector)
    }

    /// Draws the art over a box of `size` at the surface's origin.
    pub(super) fn draw(&self, surface: &mut Surface, size: Size) {
        let (width, height) = (self.0.size().width(), self.0.size().height());
        let Some(bounds) = Rect::from_xywh(0.0, 0.0, width, height) else {
            return;
        };
        let (fill, stroke) = (surface.get_fill().cloned(), surface.get_stroke().cloned());
        surface.push_transform(&Transform::from_scale(
            size.width() / width,
            size.height() / height,
        ));
        surface.push_clip_path(&rectangle(bounds), &FillRule::NonZero);
        group(self.0.root(), surface);
        surface.pop();
        surface.pop();
        surface.set_fill(fill);
        surface.set_stroke(stroke);
    }
}

fn node(node: &usvg::Node, surface: &mut Surface) {
    match node {
        usvg::Node::Group(inner) => group(inner, surface),
        usvg::Node::Path(inner) => path(inner, surface),
        usvg::Node::Image(inner) => image(inner, surface),
        usvg::Node::Text(_) => {}
    }
}

fn group(group: &usvg::Group, surface: &mut Surface) {
    let mut pushed = 3;
    if group.isolate() {
        surface.push_isolated();
        pushed += 1;
    }
    surface.push_transform(&transform(group.transform()));
    if let Some(clip) = group.clip_path() {
        pushed += clip_path(clip, surface);
    }
    if let Some(mask) = group.mask() {
        pushed += self::mask(mask, surface);
    }
    surface.push_blend_mode(blend_mode(group.blend_mode()));
    surface.push_opacity(normalized(group.opacity().get()));
    for child in group.children() {
        node(child, surface);
    }
    for _ in 0..pushed {
        surface.pop();
    }
}

fn path(path: &usvg::Path, surface: &mut Surface) {
    if !path.is_visible() {
        return;
    }
    let Some(outline) = outline(path.data().segments(), usvg::Transform::identity()) else {
        return;
    };
    let mut draw = |fill: Option<&usvg::Fill>, stroke: Option<&usvg::Stroke>| {
        let fill = fill.map(|fill| self::fill(fill, surface));
        let stroke = stroke.map(|stroke| self::stroke(stroke, surface));
        // With neither set, krilla fills in black.
        if fill.is_some() || stroke.is_some() {
            surface.set_fill(fill);
            surface.set_stroke(stroke);
            surface.draw_path(&outline);
        }
    };
    match path.paint_order() {
        usvg::PaintOrder::FillAndStroke => draw(path.fill(), path.stroke()),
        usvg::PaintOrder::StrokeAndFill => {
            draw(None, path.stroke());
            draw(path.fill(), None);
        }
    }
}

/// An image the SVG carries inside it: pixels as an image object, and
/// another SVG as its own paths.
fn image(image: &usvg::Image, surface: &mut Surface) {
    if !image.is_visible() {
        return;
    }
    let data = |bytes: &Arc<Vec<u8>>| krilla::Data::from(bytes.as_ref().clone());
    let raster = match image.kind() {
        usvg::ImageKind::JPEG(bytes) => Image::from_jpeg(data(bytes), false),
        usvg::ImageKind::PNG(bytes) => Image::from_png(data(bytes), false),
        usvg::ImageKind::GIF(bytes) => Image::from_gif(data(bytes), false),
        usvg::ImageKind::WEBP(bytes) => Image::from_webp(data(bytes), false),
        usvg::ImageKind::SVG(tree) => {
            let Some(bounds) = Rect::from_xywh(0.0, 0.0, tree.size().width(), tree.size().height())
            else {
                return;
            };
            surface.push_clip_path(&rectangle(bounds), &FillRule::NonZero);
            group(tree.root(), surface);
            surface.pop();
            return;
        }
    };
    if let (Some(raster), Some(size)) = (
        raster,
        Size::from_wh(image.size().width(), image.size().height()),
    ) {
        surface.draw_image(raster, size);
    }
}

/// Pushes one clip path and reports how many pops undo it.
///
/// A PDF clip is one path under one fill rule. A clip path whose
/// shapes fit that is pushed as a clip. One that does not, because a
/// shape inside it is clipped again or its shapes disagree about the
/// rule, is drawn as an alpha mask.
fn clip_path(clip: &usvg::ClipPath, surface: &mut Surface) -> u16 {
    let mut rules = Vec::new();
    clip_rules(clip.root(), &mut rules);
    let one_rule = rules.iter().all(|rule| *rule == usvg::FillRule::NonZero)
        || (rules.len() == 1 && rules[0] == usvg::FillRule::EvenOdd);
    if !(plain(clip.root()) && one_rule) {
        let mut builder = surface.stream_builder();
        let mut inner = builder.surface();
        let mut pushed = 1;
        if let Some(parent) = clip.clip_path() {
            pushed += clip_path(parent, &mut inner);
        }
        inner.push_transform(&transform(clip.transform()));
        group(clip.root(), &mut inner);
        for _ in 0..pushed {
            inner.pop();
        }
        inner.finish();
        let stream = builder.finish();
        surface.push_mask(Mask::new(stream, MaskType::Alpha));
        return 1;
    }
    let mut pushed = match clip.clip_path() {
        Some(parent) => clip_path(parent, surface),
        None => 0,
    };
    let mut builder = PathBuilder::new();
    clip_segments(clip.root(), clip.transform(), &mut builder);
    // A clip path with nothing in it hides everything it clips.
    let path = builder.finish().unwrap_or_else(|| {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, 0.0);
        builder.line_to(0.0, 0.0);
        builder.finish().expect("a line is a path")
    });
    let rule = rules.first().copied().unwrap_or(usvg::FillRule::NonZero);
    surface.push_clip_path(&path, &fill_rule(rule));
    pushed += 1;
    pushed
}

/// Whether no shape inside a clip path is clipped again.
fn plain(group: &usvg::Group) -> bool {
    group.children().iter().all(|child| match child {
        usvg::Node::Group(inner) => inner.clip_path().is_none() && plain(inner),
        _ => true,
    })
}

fn clip_rules(group: &usvg::Group, rules: &mut Vec<usvg::FillRule>) {
    for child in group.children() {
        match child {
            usvg::Node::Path(path) => rules.extend(path.fill().map(usvg::Fill::rule)),
            usvg::Node::Group(inner) => clip_rules(inner, rules),
            _ => {}
        }
    }
}

fn clip_segments(group: &usvg::Group, transform: usvg::Transform, builder: &mut PathBuilder) {
    for child in group.children() {
        match child {
            usvg::Node::Path(path) if path.is_visible() => {
                segments(path.data().segments(), transform, builder);
            }
            usvg::Node::Group(inner) => {
                clip_segments(inner, transform.pre_concat(inner.transform()), builder);
            }
            _ => {}
        }
    }
}

/// Pushes one mask and reports how many pops undo it.
fn mask(mask: &usvg::Mask, surface: &mut Surface) -> u16 {
    let mut builder = surface.stream_builder();
    let mut inner = builder.surface();
    let mut pushed = 0;
    if let Some(parent) = mask.mask() {
        pushed += self::mask(parent, &mut inner);
    }
    let bounds = mask.rect().to_rect();
    if let Some(bounds) =
        Rect::from_ltrb(bounds.left(), bounds.top(), bounds.right(), bounds.bottom())
    {
        inner.push_clip_path(&rectangle(bounds), &FillRule::NonZero);
        pushed += 1;
    }
    group(mask.root(), &mut inner);
    for _ in 0..pushed {
        inner.pop();
    }
    inner.finish();
    let kind = match mask.kind() {
        usvg::MaskType::Luminance => MaskType::Luminosity,
        usvg::MaskType::Alpha => MaskType::Alpha,
    };
    let stream = builder.finish();
    surface.push_mask(Mask::new(stream, kind));
    1
}

fn fill(fill: &usvg::Fill, surface: &mut Surface) -> Fill {
    Fill {
        paint: paint(fill.paint(), surface),
        opacity: normalized(fill.opacity().get()),
        rule: fill_rule(fill.rule()),
    }
}

fn stroke(stroke: &usvg::Stroke, surface: &mut Surface) -> Stroke {
    Stroke {
        paint: paint(stroke.paint(), surface),
        width: stroke.width().get(),
        miter_limit: stroke.miterlimit().get(),
        line_cap: match stroke.linecap() {
            usvg::LineCap::Butt => LineCap::Butt,
            usvg::LineCap::Round => LineCap::Round,
            usvg::LineCap::Square => LineCap::Square,
        },
        line_join: match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => LineJoin::Miter,
            usvg::LineJoin::Round => LineJoin::Round,
            usvg::LineJoin::Bevel => LineJoin::Bevel,
        },
        opacity: normalized(stroke.opacity().get()),
        dash: stroke.dasharray().map(|array| StrokeDash {
            offset: stroke.dashoffset(),
            array: array.to_vec(),
        }),
    }
}

fn paint(paint: &usvg::Paint, surface: &mut Surface) -> Paint {
    let spread = |method: usvg::SpreadMethod| match method {
        usvg::SpreadMethod::Pad => SpreadMethod::Pad,
        usvg::SpreadMethod::Reflect => SpreadMethod::Reflect,
        usvg::SpreadMethod::Repeat => SpreadMethod::Repeat,
    };
    let stops = |stops: &[usvg::Stop]| -> Vec<Stop> {
        stops
            .iter()
            .map(|stop| Stop {
                offset: normalized(stop.offset().get()),
                color: rgb::Color::new(stop.color().red, stop.color().green, stop.color().blue)
                    .into(),
                opacity: normalized(stop.opacity().get()),
            })
            .collect()
    };
    match paint {
        usvg::Paint::Color(color) => rgb::Color::new(color.red, color.green, color.blue).into(),
        usvg::Paint::LinearGradient(gradient) => LinearGradient {
            x1: gradient.x1(),
            y1: gradient.y1(),
            x2: gradient.x2(),
            y2: gradient.y2(),
            transform: transform(gradient.transform()),
            spread_method: spread(gradient.spread_method()),
            stops: stops(gradient.stops()),
            anti_alias: false,
        }
        .into(),
        usvg::Paint::RadialGradient(gradient) => RadialGradient {
            cx: gradient.cx(),
            cy: gradient.cy(),
            cr: gradient.r().get(),
            fx: gradient.fx(),
            fy: gradient.fy(),
            fr: 0.0,
            transform: transform(gradient.transform()),
            spread_method: spread(gradient.spread_method()),
            stops: stops(gradient.stops()),
            anti_alias: false,
        }
        .into(),
        usvg::Paint::Pattern(pattern) => {
            let mut builder = surface.stream_builder();
            let mut inner = builder.surface();
            group(pattern.root(), &mut inner);
            inner.finish();
            Pattern {
                stream: builder.finish(),
                transform: transform(pattern.transform().pre_concat(
                    usvg::Transform::from_translate(pattern.rect().x(), pattern.rect().y()),
                )),
                width: pattern.rect().width(),
                height: pattern.rect().height(),
            }
            .into()
        }
    }
}

fn fill_rule(rule: usvg::FillRule) -> FillRule {
    match rule {
        usvg::FillRule::NonZero => FillRule::NonZero,
        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
    }
}

fn blend_mode(mode: usvg::BlendMode) -> BlendMode {
    match mode {
        usvg::BlendMode::Normal => BlendMode::Normal,
        usvg::BlendMode::Multiply => BlendMode::Multiply,
        usvg::BlendMode::Screen => BlendMode::Screen,
        usvg::BlendMode::Overlay => BlendMode::Overlay,
        usvg::BlendMode::Darken => BlendMode::Darken,
        usvg::BlendMode::Lighten => BlendMode::Lighten,
        usvg::BlendMode::ColorDodge => BlendMode::ColorDodge,
        usvg::BlendMode::ColorBurn => BlendMode::ColorBurn,
        usvg::BlendMode::HardLight => BlendMode::HardLight,
        usvg::BlendMode::SoftLight => BlendMode::SoftLight,
        usvg::BlendMode::Difference => BlendMode::Difference,
        usvg::BlendMode::Exclusion => BlendMode::Exclusion,
        usvg::BlendMode::Hue => BlendMode::Hue,
        usvg::BlendMode::Saturation => BlendMode::Saturation,
        usvg::BlendMode::Color => BlendMode::Color,
        usvg::BlendMode::Luminosity => BlendMode::Luminosity,
    }
}

fn normalized(value: f32) -> NormalizedF32 {
    NormalizedF32::new(value).unwrap_or(NormalizedF32::ONE)
}

fn transform(transform: usvg::Transform) -> Transform {
    Transform::from_row(
        transform.sx,
        transform.ky,
        transform.kx,
        transform.sy,
        transform.tx,
        transform.ty,
    )
}

fn rectangle(rect: Rect) -> Path {
    let mut builder = PathBuilder::new();
    builder.push_rect(rect);
    builder.finish().expect("a rectangle is a path")
}

fn outline(from: impl Iterator<Item = PathSegment>, transform: usvg::Transform) -> Option<Path> {
    let mut builder = PathBuilder::new();
    segments(from, transform, &mut builder);
    builder.finish()
}

fn segments(
    from: impl Iterator<Item = PathSegment>,
    transform: usvg::Transform,
    builder: &mut PathBuilder,
) {
    for segment in from {
        match segment {
            PathSegment::MoveTo(to) => {
                let mut points = [to];
                transform.map_points(&mut points);
                builder.move_to(points[0].x, points[0].y);
            }
            PathSegment::LineTo(to) => {
                let mut points = [to];
                transform.map_points(&mut points);
                builder.line_to(points[0].x, points[0].y);
            }
            PathSegment::QuadTo(control, to) => {
                let mut points = [control, to];
                transform.map_points(&mut points);
                builder.quad_to(points[0].x, points[0].y, points[1].x, points[1].y);
            }
            PathSegment::CubicTo(first, second, to) => {
                let mut points = [first, second, to];
                transform.map_points(&mut points);
                builder.cubic_to(
                    points[0].x,
                    points[0].y,
                    points[1].x,
                    points[1].y,
                    points[2].x,
                    points[2].y,
                );
            }
            PathSegment::Close => builder.close(),
        }
    }
}
