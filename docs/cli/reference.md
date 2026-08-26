---
title: CLI reference
description: Flags, repeatable -c, exit codes, and what lands on stderr.
---

```text
usage: fleuron <input.json> -o <output.pdf> [-c <style.css>]

  -o, --output <path>  where to write the PDF
  -c, --css <path>     author stylesheet, cascading over the defaults;
                       repeatable, applied in the order given
  -V, --version        print the version and exit
  -h, --help           print this message and exit
```

## Arguments

**`<input.json>`** — a content tree, as JSON. Exactly one, in any position. Two positional arguments is a usage error rather than a batch.

**`-o`, `--output`** — where the PDF goes. Required. The path is written whole; nothing is created alongside it.

**`-c`, `--css`** — an author stylesheet. Repeatable. Sheets are parsed in the order given and cascade in that order, all of them over the built-in user-agent sheet. Omitting it entirely is legal and gives you the built-in design.

**`-V`, `--version`** — prints `fleuron <version>` and exits 0, whether or not a job was named.

**`-h`, `--help`** — prints the usage above and exits 0, whether or not a job was named.

An unrecognised option beginning with `-` is a usage error. There is no `--` separator, because there is nothing after the input a stylesheet could be confused with.

## Exit codes

| code | meaning |
|---|---|
| 0 | The PDF was written. Warnings do not change this. |
| 1 | The job was named and failed. Nothing was written. |
| 2 | The command line named no job to do. |

The distinction between 1 and 2 is whether the fault was in the invocation or in the work: 2 means fix the command, 1 means fix the input. A book that laid out with complaints exits 0, having said so — warnings are not failures, and a build that wants them to be should check stderr.

## stderr

Everything the run has to say goes to stderr; stdout carries only what `--version` and `--help` print. A summary comes first:

```text
fleuron: book.json → book.pdf: 333 pages
```

Then each warning, prefixed with its origin when it has one:

```text
fleuron: warning: house.css:14:3: unsupported property: color
fleuron: warning: chapter-03.md:88:1: family "House Sans" resolved nothing
fleuron: 2 warnings; the PDF was written anyway
```

The origin is a CSS sheet with a line and column, or the frontend's own source file and position for a content node. [Diagnostics](../library/diagnostics.md) covers what warns and why.

## Fonts on the command line

`@font-face` urls are treated as file paths. Each is tried against the directory of every stylesheet given with `-c`, in order, and then against the working directory. The engine itself opens nothing — this resolution is the binary's, and a library embedding fleuron makes its own rules.
