---
title: Install
description: How to install fleuron as a Rust library, a command-line binary, or an npm package.
---

Fleuron is not on crates.io, so the Rust routes below point at the repository. You need Rust 1.85 or newer, and the workspace is on the 2024 edition.

## As a Rust library

`fleuron` is the engine and `fleuron-markdown` is the frontend that reads markdown into it. Add both:

```toml
# Cargo.toml
[dependencies]
fleuron = { git = "https://github.com/zachhannum/fleuron" }
fleuron-markdown = { git = "https://github.com/zachhannum/fleuron" }
```

`fleuron-markdown` is optional if you build a [content tree](reference/content-tree.md) yourself.

Next: the [library quickstart](library/quickstart.md).

## As a command-line binary

```sh
cargo install --git https://github.com/zachhannum/fleuron fleuron-cli
```

That puts a `fleuron` binary on your path. It reads markdown and writes a PDF, and takes author stylesheets through repeatable `-c` flags. The following command sets `manuscript.md` with one stylesheet:

```sh
fleuron manuscript.md -o book.pdf -c book.css
```

Next: the [CLI quickstart](cli/quickstart.mdx).

## As an npm package

```sh
npm install fleuron
```

You get the module, a worker, a client, a display-structure reader, and an SVG painter.

```sh
npm install fleuron-react
```

`fleuron-react` is the same preview as a React component. It contains no engine logic of its own.

Next: the [WebAssembly quickstart](wasm/quickstart.md), or [the demos](https://fleuron.typeworks.dev/demos/), which run this package in your browser.

## Working on fleuron itself

```sh
git clone https://github.com/zachhannum/fleuron
cd fleuron
cargo test --workspace
```

The end-to-end test runs the fixture book through the CLI and checks the PDF it wrote. It needs `qpdf` and `pdftotext`, from poppler, on the path. Without them it skips the validation rather than failing.
