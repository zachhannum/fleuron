---
title: Install
description: How to install fleuron as a Rust library, a command-line binary, or an npm package.
---

fleuron is not on crates.io yet, so the Rust routes below point at the repository. When it publishes, the git dependency becomes a version and nothing else changes.

You need Rust 1.85 or newer. The workspace is on the 2024 edition.

## As a Rust library

```toml
# Cargo.toml
[dependencies]
fleuron = { git = "https://github.com/zachhannum/fleuron" }
fleuron-markdown = { git = "https://github.com/zachhannum/fleuron" }
```

`fleuron-markdown` reads markdown into sections. Leave it out if your source is already structured and you build a `Book` yourself.

Nothing else is required. The engine bundles EB Garamond as its default text face, does no I/O, and links no platform libraries, so it builds for `wasm32-unknown-unknown` unchanged.

Next: the [library quickstart](library/quickstart.md).

## As a command-line binary

```sh
cargo install --git https://github.com/zachhannum/fleuron fleuron-cli
```

That puts a `fleuron` binary on your path. It reads markdown and writes a PDF. Author stylesheets go in through repeatable `-c` flags.

```sh
fleuron manuscript.md -o book.pdf -c book.css
```

Next: the [CLI quickstart](cli/quickstart.mdx).

## As an npm package

```sh
npm install @fleuron/wasm
```

You get the module, a worker, a client, a display-list reader and an SVG painter. Layout runs off the main thread and nothing in the package touches the DOM.

```sh
npm install @fleuron/react
```

That is the same preview as a React component, and nothing else.

Next: the [wasm quickstart](wasm/quickstart.md), or [the demos](https://fleuron.typeworks.dev/demos/), which run this package in your browser.

## Working on fleuron itself

```sh
git clone https://github.com/zachhannum/fleuron
cd fleuron
cargo test --workspace
```

The end-to-end test runs the fixture book through the CLI and checks the PDF. It wants `qpdf` and `pdftotext` (from poppler) on the path. Without them it skips the validation instead of failing.

The perf harness is a separate binary, and it reports against absolute ceilings rather than a stored baseline:

```sh
cargo run --release -p fleuron-fixtures --bin perf-gate
cargo bench -p fleuron
```
