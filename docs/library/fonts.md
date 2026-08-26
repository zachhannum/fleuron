---
title: Fonts
description: Registering faces, family matching, and what a missing face does.
---

Every glyph the engine draws comes from a face in a `FontRegistry`. There are two ways in — the bundled default, and `@font-face` — and one rule about the boundary: the engine reads no paths of its own.

## The bundled registry

```rust
let mut registry = fleuron::fonts::bundled_registry()?;
```

That is EB Garamond, upright and italic, registered as the generic `serif` and as the built-in sheet's default family. It is a variable file, so the family holds every named cut the file declares rather than a single weight. A book that adds no fonts of its own still sets in a real book face.

## Adding faces from a stylesheet

```css
@font-face {
  font-family: "Author Serif";
  src: url("faces/author.ttf") format("truetype");
  font-style: normal;
  font-weight: 400;
}
```

`src` urls mean whatever the host says they mean. The engine hands each one to the `FontLoader` it was given:

```rust
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

sheets.load_fonts(&mut registry, &Files(PathBuf::from("assets")));
```

A host that resolves nothing is a host with no author fonts — `NoFonts` is exactly that, and is the right loader in a sandbox. Load fonts before compiling: a face the registry does not hold is a face no computed style can resolve to.

A face that resolves is registered under the family the *sheet* declared, not the one inside the file. A stylesheet's name for a face is the name its selectors use, and a file whose internal name disagrees is not a special case.

## Variable files

A variable file registers one face per named instance, so a family declared once holds every cut the file names.

Declaring `font-style` or `font-weight` on the `@font-face` rule overrides that: the sheet has said what this source is, and it registers as that one cut. Use it when a file names instances you do not want, or when you are pinning one weight of a family you are assembling from several files.

## Matching

`font-family` is tried in order, and the first registered family answers. Nothing later in the list is consulted once one has.

Within a family, **slope decides before weight**. A family with an italic cut never answers an italic request with the upright one, whatever the weights available. Weight then follows CSS: between 400 and 500 the search looks up before it looks down, and outside that range it looks away from it.

## Nothing is synthesised

There is no oblique-by-shearing and no bold-by-smearing. Two consequences, and both of them warn:

A family with no italic cut lays out upright, and says so. A stack that resolves nothing falls back to the first registered face, and says that. In both cases the book still lays out — a missing face is a diagnostic, not a failure. See [diagnostics](diagnostics.md).

## Metrics

`registry.metrics(font_id)` gives units per em, ascender, descender and line gap in font units, taken from the OS/2 typographic values when the face has them and hhea otherwise. Descender is negative, as the tables have it.

These are what `line-height: normal` resolves against. A face whose vertical metrics disagree with its optical size will set loose or tight, and the fix is a `line-height` in the stylesheet rather than anything in the engine.
