---
title: Install
description: The three ways into fleuron, and what each one needs.
---

fleuron is not on crates.io yet, so every route below points at the repository. When it publishes, the git dependency becomes a version and nothing else here changes.

Rust 1.85 or newer — the workspace is on the 2024 edition.

## As a Rust library

```toml
# Cargo.toml
[dependencies]
fleuron = { git = "https://github.com/zachhannum/fleuron" }
fleuron-markdown = { git = "https://github.com/zachhannum/fleuron" }
```

`fleuron-markdown` is the frontend: markdown in, sections out. Leave it out if your source is already structured and you build a `Book` yourself.

Nothing else is required. The engine bundles EB Garamond as its default text face, does no I/O of its own, and pulls in no platform libraries, so it builds for `wasm32-unknown-unknown` unchanged.

Go on to the [library quickstart](library/quickstart.md).

## As a command-line binary

```sh
cargo install --git https://github.com/zachhannum/fleuron fleuron-cli
```

That puts a `fleuron` binary on your path. It reads markdown and writes a PDF; author stylesheets are supplied with repeatable `-c` flags.

```sh
fleuron manuscript.md -o book.pdf -c book.css
```

Go on to the [CLI quickstart](cli/quickstart.mdx).

## As a WebAssembly module

```sh
npm install @fleuron/wasm
```

The module, a worker, a client, a reader for the display list and an SVG painter over it. Layout runs off the main thread and nothing in the package touches the DOM.

```sh
npm install @fleuron/react
```

The same preview as a React component, and nothing else. Go on to the [wasm quickstart](wasm/quickstart.md), or to [the demos](https://zachhannum.github.io/fleuron/demos/), which run this package in your browser.

## Working on fleuron itself

```sh
git clone https://github.com/zachhannum/fleuron
cd fleuron
cargo test --workspace
```

The end-to-end test is fixture book in, valid PDF out, through the CLI. It wants `qpdf` and `pdftotext` (from poppler) on the path to validate the result; without them it skips the validation instead of failing.

The perf harness is a separate binary, and it reports against absolute ceilings rather than a stored baseline:

```sh
cargo run --release -p fleuron-fixtures --bin perf-gate
cargo bench -p fleuron
```
