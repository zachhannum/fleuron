---
title: Fonts
description: Registering faces, family matching, and what a missing face does.
---

Every glyph the engine draws comes from a face in a `FontRegistry`. Faces arrive two ways: the bundled default, and `@font-face`. The engine reads no paths of its own either way.

## The bundled registry

```rust
let mut registry = fleuron::fonts::bundled_registry()?;
```

That is EB Garamond, upright and italic, registered as the generic `serif` and as the built-in sheet's default family. It is a variable file, so the family holds every named cut the file declares. A book that adds no fonts still has an upright and an italic to set in.

## Adding faces from a stylesheet

```css
@font-face {
  font-family: "Author Serif";
  src: url("faces/author.ttf") format("truetype");
  font-style: normal;
  font-weight: 400;
}
```

The engine never resolves a `src` url. It hands each one to the `FontLoader` it was given, exactly as the sheet wrote it, and whether that string is a path, a key or a URL is the loader's business:

```rust
struct Files(PathBuf);

impl FontLoader for Files {
    fn load(&self, url: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(url)).ok()
    }
}

sheets.load_fonts(&mut registry, &Files(PathBuf::from("assets")));
```

A host that resolves nothing has no author fonts. `NoFonts` is that loader, and the one to use in a sandbox.

Load fonts before compiling. A computed style can only resolve to a face the registry already holds. A [session](sessions.md) borrows the registry for its whole life, so under one, loading happens before the session is made.

A face registers under the family the *sheet* declared, not the name inside the file, so a sheet's selectors always match the name the sheet wrote.

## Variable files

A variable file registers one face per named instance, so a family declared once holds every cut the file names.

Declaring `font-style` or `font-weight` on the `@font-face` rule overrides that, and the source registers as that one cut. Use it when a file names instances you do not want, or when you are pinning one weight of a family you are assembling from several files.

## Matching

`font-family` is tried in order and the first registered family answers. Nothing later in the list is consulted once one has.

Within a family, slope decides before weight. A family with an italic cut never answers an italic request with the upright one, whatever weights are available. Weight then follows CSS: between 400 and 500 the search looks up before it looks down, and outside that range it looks away from it.

Nothing is synthesised. There is no oblique-by-shearing and no bold-by-smearing. A family with no italic cut lays out upright, and a stack that resolves nothing falls back to the first registered face. Both warn, naming what was wanted, and the book lays out either way. See [diagnostics](diagnostics.mdx).

## Metrics

`registry.metrics(font_id)` gives units per em, ascender, descender and line gap in font units. They come from the OS/2 typographic values when the face has them, and hhea otherwise. Descender is negative, as the tables have it.

These are what `line-height: normal` resolves against. A face whose vertical metrics disagree with its optical size sets loose or tight, and the fix is a `line-height` in the stylesheet.
