---
title: Fonts
description: Registering faces, family matching, and what a missing face does.
---

Every glyph the engine draws comes from a font face in a `FontRegistry`. The engine comes 
with bundled default, EB Garamond. Additional fonts can be configured and defined via `@font-face`.

## The bundled registry

```rust
let mut registry = fleuron::fonts::bundled_registry()?;
```

The bundled registry includes EB Garamond, upright and italic, registered as the generic 
`serif` and as the built-in sheet's default family. It is a variable font file.

## Adding faces from a stylesheet

```css
@font-face {
  font-family: "Author Serif";
  src: url("faces/author.ttf") format("truetype");
  font-style: normal;
  font-weight: 400;
}
```

Whatever string `url()` names is handed to the `FontLoader` unchanged. It never has to be a real URL:

```rust
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

sheets.load_fonts(&mut registry, &Files(PathBuf::from("assets")));
```

`NoFonts` is a loader that returns `None` for every url, for a host with nothing to read from, 
such as a sandbox with no filesystem. In this case, only the bundled fonts will be used.

```rust
use fleuron::style::NoFonts;

sheets.load_fonts(&mut registry, &NoFonts);
```

Load fonts before compiling. A computed style can only resolve to a font face the registry has 
already loaded. A [session](sessions.md) borrows the registry for its whole life.

A face registers under the family the _sheet_ declared, not the name inside the file, so a 
sheet's selectors always match the name the sheet wrote.

## Variable files

A variable file registers one face per named instance, so one declaration gives the family every style the file names.

Declaring `font-style` or `font-weight` on the `@font-face` rule overrides that, and the source 
registers as that one style. Use it when a file names instances you do not want, or when you are 
pinning one weight of a family you are assembling from several files.

```css
@font-face {
  font-family: "Author Serif";
  src: url("faces/author-regular.ttf") format("truetype");
  font-style: normal;
  font-weight: 400;
}

@font-face {
  font-family: "Author Serif";
  src: url("faces/author-italic.ttf") format("truetype");
  font-style: italic;
  font-weight: 400;
}
```

Both rules name one family, so a sheet asking for `"Author Serif"` can use both styles and an 
emphasis in the document resolves to the italic.

## Matching

`font-family` is tried in order and the first registered family answers. Nothing later in the 
list is consulted once one has.

```css
body {
  font-family: "Author Serif", "Fallback Serif", serif;
}
```

If `"Author Serif"` is registered it answers, and `"Fallback Serif"` is never consulted, 
even when the emphasis on the page needs an italic that `"Author Serif"` has no style for. 
The stack names families to try in order, and the one that is selected must supply the style.

Within a family, slope decides before weight. A family with an italic style never answers 
an italic request with the upright one, whatever weights are available. Weight then follows
CSS: between 400 and 500 the search looks up before it looks down, and outside that range it looks away from it.

Nothing is synthesised. There is no oblique-by-shearing and no bold-by-smearing. A family 
with no italic cut lays out upright, and a stack that resolves nothing falls back to the 
first registered face. Both warn, naming what was asked for, and the book lays out either way. 
See [diagnostics](diagnostics.mdx).

## Metrics

`registry.select` resolves a family and the attributes wanted to a face, and `registry.metrics` 
gives that face's units per em, ascender, descender, line gap and cap height in font units.

```rust
use fleuron::fonts::FaceAttributes;

let face = registry.select("Author Serif", FaceAttributes::REGULAR)?;
let metrics = registry.metrics(face.id)?;

// Font units to points, at 11pt.
let scale = 11.0 / f32::from(metrics.units_per_em);
let ascender = f32::from(metrics.ascender) * scale;
```

Both return `None` for a family the registry does not contain. `face.attributes` is what 
was found rather than what was asked for, so a caller can see that a family answered a 
request for italic with an upright.

Ascender, descender and line gap come from the OS/2 typographic values when the face has 
them, and hhea otherwise. Descender is negative, which is how the tables record it. Cap height is zero when the face declares none.

These are what `line-height: normal` resolves against. A face whose vertical metrics 
disagree with its optical size sets loose or tight, and the fix is a `line-height` in the stylesheet.
