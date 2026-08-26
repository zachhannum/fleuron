---
title: Diagnostics
description: The Warning channel — what warns, what fails, and what to do about it.
---

fleuron has two ways of telling you something is wrong, and the line between them is worth learning, because almost everything falls on one side of it.

**A warning is a book that laid out anyway.** Unsupported CSS, a font that would not load, a family that resolved nothing. The run finishes, the PDF is written, and the warning names where the problem was written.

**An error is a book that did not.** Malformed input, a face that cannot be embedded, geometry a PDF cannot express. There is no output to look at.

## Reading warnings

Warnings come back on the output, not through a channel you have to subscribe to:

```rust
let output = fleuron::layout::layout_book(&book, &styles, &registry);
for warning in &output.warnings {
    match &warning.origin {
        Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
        None => eprintln!("warning: {}", warning.message),
    }
}
```

`origin` is a source location when one exists. For CSS that is the sheet's name with a line and column — the name being whatever you passed to `Source::author`, so pass something a reader will recognise. For content it is the frontend's own file and position, `chapter-01.md:12:3`, carried on the node from the markdown it was parsed out of. A node with no position degrades to the bare file name rather than losing the warning.

Style compilation collects its own warnings before layout runs, and `Stylesheets::warnings()` has them as soon as parsing and font loading are done. `LayoutOutput::warnings` is the whole run's, compilation included, so reading it once at the end is enough.

## What warns

**Unsupported CSS.** Every declaration outside [the subset](../css-subset.md) warns, one warning per declaration, at the line and column it was written at. This is the point of having a written subset: `color: red` is not silently dropped, it is reported as not being a thing this engine does.

**A font that would not load.** An `@font-face` whose `src` the loader could not resolve, or resolved to bytes that are not a font this build can read. Text falls back to the next family in the list.

**A family that resolved nothing.** The whole `font-family` stack came up empty and the first registered face answered instead. Almost always a spelling mistake or a loader rooted in the wrong directory.

**A missing cut.** An italic or a weight the family does not have. Nothing is synthesised, so the text sets in the nearest cut and the warning says which one was wanted.

## What fails

**Input that is not a content tree.** JSON that does not deserialize. This is the frontend's error, not the book's.

**A face that could not be embedded.** `PdfError::Font`, naming the face. Usually a font whose licence bits or table layout the writer refuses.

**Geometry a PDF cannot express.** `PdfError::Geometry`, naming the folio and the kind of item. A non-finite coordinate reaching the writer is the usual cause, and it is a bug rather than a document problem.

**Serialization.** `PdfError::Serialize`, with whatever the writer said.

## From the command line

The CLI prints warnings to stderr, one per line, prefixed with the input they came from, and then a count. It still exits 0 — the PDF was written. See [the CLI reference](../cli/reference.md) for exactly which exit code means what.

## A warning you should not ignore

Warnings are non-fatal by design, so that a manuscript is never held hostage to a stylesheet. That makes them easy to let accumulate. Two are worth treating as errors in a build:

A font warning changes what the book looks like. Text that fell back is text set in the wrong face on every page it appears.

A CSS warning means a rule you wrote had no effect. Nothing downstream will ever mention it again.
