---
title: Diagnostics
description: The Warning channel — what warns, what fails, and what to do about it.
---

fleuron reports trouble two ways, and almost everything falls on the first.

**A warning is a book that laid out anyway.** Unsupported CSS, a font that would not load, a family that resolved nothing. The run finishes, the PDF is written, and the warning names where the problem was written.

**An error is a book that did not.** Malformed input, a face that cannot be embedded, geometry a PDF cannot express.

## Reading warnings

Warnings come back on the output:

```rust
let output = fleuron::layout::layout_book(&book, &styles, &registry);
for warning in &output.warnings {
    match &warning.origin {
        Some(origin) => eprintln!("warning: {origin}: {}", warning.message),
        None => eprintln!("warning: {}", warning.message),
    }
}
```

`origin` is a source location when one exists. For CSS that is the sheet's name with a line and column, the name being whatever you passed to `Source::author`, so pass something a reader will recognise. For content it is the source file and position, `chapter-01.md:12:3`, carried on the node from the markdown it was read out of. A node with no position degrades to the bare file name, and the warning is not dropped.

Style compilation collects its own warnings before layout runs, and `Stylesheets::warnings()` has them as soon as parsing and font loading are done. `LayoutOutput::warnings` is the whole run's, compilation included, so reading it once at the end is enough.

## What warns

**A construct the content vocabulary has no room for.** A list, a table, a code block: the frontend sets them as prose and names the line and column they were written at. Prose is never dropped, so a warning here says the page has changed shape rather than lost anything. [The markdown mapping](../reference/markdown.md) is the full list.

**Unsupported CSS.** Every declaration outside [the subset](../css-subset.mdx) warns, one warning per declaration, at the line and column it was written at. That is what writing the subset down buys: `color: red` is reported, not silently dropped.

**A font that would not load.** An `@font-face` whose `src` the loader could not resolve, or resolved to bytes that are not a font this build can read. Text falls back to the next family in the list.

**A family that resolved nothing.** The whole `font-family` stack came up empty and the first registered face answered instead. Almost always a spelling mistake or a loader rooted in the wrong directory.

**A missing cut.** An italic or a weight the family does not have. Nothing is synthesised, so the text sets in the nearest cut and the warning says which one was wanted.

## What fails

**Input that is not markdown.** A file the CLI cannot place by extension. This is the caller's error, not the book's.

**A face that could not be embedded.** `PdfError::Font`, naming the face. Usually a font whose licence bits or table layout the writer refuses.

**Geometry a PDF cannot express.** `PdfError::Geometry`, naming the folio and the kind of item. A non-finite coordinate reaching the writer is the usual cause, and it is a bug in the engine rather than a problem with the document.

**Serialization.** `PdfError::Serialize`, with whatever the writer said.

## From the command line

The CLI prints warnings to stderr, one per line, prefixed with the input they came from, and then a count. It still exits 0, because the PDF was written. See [the CLI reference](../cli/reference.md) for exactly which exit code means what.

## Warnings worth failing a build over

Warnings are non-fatal so that a mistake in a stylesheet never stops a manuscript from laying out, which is also what makes them easy to accumulate. Two are worth promoting to errors in a build.

A font warning changes what the book looks like. Text that fell back is set in the wrong face on every page it appears on.

A CSS warning means a rule you wrote had no effect, and nothing downstream will mention it again.

A frontend warning is the third candidate, and the one to weigh: a table set as one paragraph per cell is a page that no longer says what the manuscript meant.
