---
title: Display list
description: The engine's only product, and the contract every painter shares.
---

The display list is what the engine produces. Painters consume it and never re-derive layout, which is why a preview and an export cannot disagree: they are two renderings of the same numbers.

Coordinates are page units — points — with the origin at the top left of the trimmed page.

## `LayoutOutput`

```rust
pub struct LayoutOutput {
    pub pages: Vec<Page>,
    pub fonts: Vec<FontRefEntry>,
    pub warnings: Vec<Warning>,
}
```

`fonts` is the table `font_id` indexes. Both painters and the PDF writer resolve ids through it, so a run's face is the same face on both sides. An entry carries the family, the face and style names, the slope and weight it answers for, and `variations`: where on its file's axes it sits, in user space. A variable file's named cuts are one file at several locations, and the location is what tells them apart. `warnings` is the whole run's, style compilation included — see [diagnostics](../library/diagnostics.md).

## `Page`

```rust
pub struct Page {
    pub number: u32,
    pub side: Side,
    pub width: f32,
    pub height: f32,
    pub items: Vec<DrawItem>,
}
```

`number` is the folio, counting from 1. `side` is `recto` or `verso`; books open on a right-hand page, so odd numbers are recto. `width` and `height` are the trim, in points — per page, because a book may change page size at a named page.

`items` is in paint order.

## `DrawItem`

Deliberately tiny: three variants, and a painter that handles all three can paint any page the engine produces.

### `Text`

```rust
Text {
    x: f32,
    y: f32,
    font_id: u16,
    size: f32,
    text: String,
    glyphs: Vec<Glyph>,
}
```

One run of shaped glyphs sharing a font, a size and a baseline. `y` is the baseline, not the top of the line box.

`text` is the string the glyphs were shaped from. It travels with the run because only the shaper knew which glyph came from which character, and a painter that wants text extraction, selection or copy-and-paste needs that correspondence back. A painter that only draws can ignore it.

### `Glyph`

```rust
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub range: Range<u32>,
}
```

`id` is a glyph id in the run's font — not a character. `x` is absolute, per glyph. Kerning and justification mean no two glyphs are uniformly spaced, so there is no advance for a painter to accumulate.

`range` is the byte range in the run's `text` that this glyph stands for. A ligature spans several characters; a decomposed cluster puts several glyphs on one range. A painter that treats the range as a mapping rather than a bijection handles both.

### `Rect`

```rust
Rect { x: f32, y: f32, w: f32, h: f32 }
```

A filled rectangle: rules, borders, backgrounds.

### `Image`

```rust
Image { x: f32, y: f32, w: f32, h: f32, asset: u32 }
```

A placed image. `asset` indexes the asset table; the pixels are the host's. Layout never decoded them, so the placement is derived from a header probe and the painter is the first thing in the pipeline to see the image itself.

## Writing a painter

The whole job is: for each page, set up a coordinate system in points with the origin top-left, then walk `items` in order.

For text, resolve `font_id` through `LayoutOutput::fonts`, pin the entry's `variations`, set the size, and place each glyph by id at its absolute `x` on the run's baseline `y`. Do not shape or kern. The engine has done both, and a painter that re-shapes will disagree with the export.

The SVG painter in `@fleuron/wasm` is one worked example, and [the preview](../wasm/preview.md) is where it is written down.
