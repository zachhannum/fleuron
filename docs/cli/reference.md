---
title: CLI reference
description: Flags, repeatable -c, exit codes, and what lands on stderr.
---

```text
usage: fleuron <input.md…> -o <output.pdf> [-c <style.css>]

  -o, --output <path>  where to write the PDF
  -c, --css <path>     author stylesheet, cascading over the defaults;
                       repeatable, applied in the order given
  -s, --split <n|none> where a markdown file's sections begin: at a
                       heading of level n or shallower, or nowhere at
                       all, one section per file (default 1)
  -d, --dialect <name> commonmark, gfm or obsidian (default commonmark)
  -m, --metadata <path> the book's title and author, for a book that is
                       several files and so has no one file to read them
                       from
  -V, --version        print the version and exit
  -h, --help           print this message and exit
```

## Arguments

**`<input.md…>`** — one or more markdown files, composed in the order given, each carrying its own name into the tree. A single `.json` argument is read as a [content tree](../reference/content-tree.md) instead; two of those, or a tree beside markdown, is a usage error. An extension that is neither is an error naming the file.

**`-s`, `--split`** — where a markdown file's sections begin. A level 1 to 6 opens a section at every heading of that level or shallower; `none` opens none, so the file is one section. Default 1.

**`-d`, `--dialect`** — which markdown is being read: `commonmark`, `gfm` or `obsidian`. See [the markdown mapping](../reference/markdown.md).

**`-m`, `--metadata`** — a file holding the book's `title`, `author` and any other fields, either bare or in a `---` block. Without it, a lone markdown input is the whole book and its frontmatter is the book's; several inputs are chapters, and each file's frontmatter stays with the section it became.

**`-o`, `--output`** — where the PDF goes. Required. The path is written whole; nothing is created alongside it.

**`-c`, `--css`** — an author stylesheet. Repeatable. Sheets are parsed in the order given and cascade in that order, all of them over the built-in user-agent sheet. Omitting it entirely is legal and gives you the built-in design.

**`-V`, `--version`** — prints `fleuron <version>` and exits 0, whether or not a job was named.

**`-h`, `--help`** — prints the usage above and exits 0, whether or not a job was named.

An unrecognised option beginning with `-` is a usage error. There is no `--` separator: every positional argument is an input, and every option takes its value next.

## Exit codes

| code | meaning |
|---|---|
| 0 | The PDF was written. Warnings do not change this. |
| 1 | The job was named and failed. Nothing was written. |
| 2 | The command line named no job to do. |

2 means fix the command; 1 means fix the input. A book that laid out with complaints exits 0, having printed them. A build that wants warnings to fail it should check stderr.

## stderr

Everything the run has to say goes to stderr; stdout carries only what `--version` and `--help` print. A summary comes first:

```text
fleuron: manuscript.md → book.pdf: 333 pages
```

Then each warning, prefixed with its origin when it has one:

```text
fleuron: warning: house.css:14:3: unsupported property: color
fleuron: warning: chapter-03.md:88:1: family "House Sans" resolved nothing
fleuron: 2 warnings; the PDF was written anyway
```

The origin is a CSS sheet with a line and column, or a markdown file and the position in it the frontend read the node from. [Diagnostics](../library/diagnostics.md) covers what warns and why.

## Fonts on the command line

`@font-face` urls are treated as file paths. Each is tried against the directory of every stylesheet given with `-c`, in order, and then against the working directory. The engine itself opens nothing: this resolution belongs to the binary, and a library embedding fleuron makes its own.
