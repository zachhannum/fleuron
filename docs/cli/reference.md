---
title: CLI reference
description: Flags, exit codes, and what lands on stderr.
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
  --title <text>       the book's title
  --author <text>      the book's author
  --meta <key=value>   any other metadata field; repeatable. `language`
                       is the one the PDF writer reads
  --dump-tree          write the content tree the frontend read to
                       stdout as JSON, and lay nothing out
  -V, --version        print the version and exit
  -h, --help           print this message and exit
```

## Arguments

| | |
|---|---|
| `<input.md…>` | One or more markdown files, composed in the order given, each recorded in the tree under its own name. An extension that is not `.md` or `.markdown` is an error naming the file. |
| `-o`, `--output` | Where the PDF goes. Required, except under `--dump-tree`. The path is written whole and nothing is created alongside it. |
| `-c`, `--css` | An author stylesheet. Repeatable. Sheets parse in the order given and cascade in that order, all of them over the built-in user-agent sheet. With no `-c`, the built-in sheet does all the styling. |
| `-s`, `--split` | Where a markdown file's sections begin. A level of 1 to 6 opens a section at every heading of that level or shallower. `none` opens none, so the file is one section. Default 1. |
| `-d`, `--dialect` | Which markdown is being read: `commonmark`, `gfm` or `obsidian`. See [the markdown mapping](../reference/markdown.mdx). |
| `--title`, `--author`, `--meta` | The book's own metadata. `--meta` takes `key=value` and is repeatable. The engine reads three fields in all: `title` and `author` become the PDF's document information, and `--meta language=en` becomes its language. |
| `--dump-tree` | Writes the [content tree](../reference/content-tree.md) the frontend read to stdout as JSON, and lays nothing out. The same manuscript dumps the same bytes every time. |
| `-V`, `--version` | Prints `fleuron <version>` and exits 0, whether or not a job was named. |
| `-h`, `--help` | Prints the usage above and exits 0, whether or not a job was named. |

A lone markdown input is the whole book, so its frontmatter fills whatever `--title`, `--author` and `--meta` leave unset. Several inputs are chapters, and each file's frontmatter stays with the section it became.

An unrecognised option beginning with `-` is a usage error. There is no `--` separator: every positional argument is an input, and every option takes its value next.

## Exit codes

| code | meaning |
|---|---|
| 0 | The PDF was written. Warnings do not change this. |
| 1 | The job was named and failed. Nothing was written. |
| 2 | The command line named no job to do. |

Exit 2 means the command line is wrong. Exit 1 means the input is. A book that laid out with warnings exits 0, so a build that must fail on warnings checks stderr.

## stderr

The run writes everything to stderr. stdout is only what `--version`, `--help` and `--dump-tree` print.

A summary comes first:

```text
fleuron: manuscript.md → book.pdf: 333 pages
```

Then each warning, prefixed with its origin when it has one:

```text
fleuron: warning: house.css:14:3: unsupported property: color
fleuron: warning: chapter-03.md:88:1: family "House Sans" resolved nothing
fleuron: 2 warnings; the PDF was written anyway
```

The origin is either a CSS sheet with a line and column, or a markdown file and the position the frontend read the node from. [Diagnostics](../library/diagnostics.mdx) covers what warns and why.

## Fonts and images on the command line

`@font-face` and image urls are treated as file paths. Each is tried against the directory of every manuscript and of every stylesheet given with `-c`, in order, and then against the working directory. The engine opens nothing itself, so this resolution belongs to the binary. A library embedding fleuron supplies its own.

An image the binary cannot open, or opens and cannot read a header from, is a warning naming the url, and the book is laid out without it.
