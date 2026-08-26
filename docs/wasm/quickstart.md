---
title: WebAssembly quickstart
description: Building the bindings and loading them in a worker.
---

:::caution[Not shipped yet]
`fleuron-wasm` has no exports today. Its `src/lib.rs` is a module doc and nothing else, and there is no `wasm-pack` artifact to load.

This page describes the contract the bindings are being written against, so that a host can be designed around it now. It will be replaced with a worked example when the bindings land. Nothing on this page has been run.
:::

## What already works

The engine runs under WebAssembly. It has no platform dependencies, opens no files, and touches no clock: `cargo build -p fleuron-wasm --target wasm32-unknown-unknown` is a CI job, and the perf harness compiles to `wasm32-wasip1` and holds both corpus novels against their budgets under wasmtime on every pull request.

So the question the bindings answer is not *can this run in a browser* — it demonstrably does — but *what shape does the door out of it take*.

## Building

```sh
cargo install wasm-pack
wasm-pack build crates/fleuron-wasm --target web --release
```

That will produce `pkg/` beside the crate: a `.wasm` module and a JavaScript shim.

## Loading it in a worker

Layout belongs off the main thread. A book-scale manuscript is hundreds of milliseconds of work, and hundreds of milliseconds on the main thread is a dropped interaction.

```js
// fleuron.worker.js
import init, { layout, export_pdf } from './pkg/fleuron_wasm.js';

const ready = init();

self.onmessage = async ({ data }) => {
  await ready;
  const bytes = data.kind === 'pdf' ? export_pdf(data.input) : layout(data.input);
  self.postMessage({ id: data.id, bytes }, [bytes]);
};
```

```js
// the host
const worker = new Worker(new URL('./fleuron.worker.js', import.meta.url), {
  type: 'module',
});

worker.postMessage({ id: 1, kind: 'layout', input }, [input.buffer]);
worker.onmessage = ({ data }) => paint(decodeDisplayList(data.bytes));
```

`input` is one postcard-encoded buffer holding the content tree, the stylesheets and the font bytes. The output is one transferable `ArrayBuffer`: the display list, or PDF bytes. Both directions transfer rather than copy, because a book's worth of glyph positions is not a thing to clone twice per keystroke.

[The wire](wire.md) has the encoding and the host's obligations.

## What the host owns

Three things the engine will not do for you, by design rather than by omission.

**Fonts.** The engine reads no paths. Font bytes cross the wire with the input; fetching, caching and versioning them are the host's.

**Images.** Layout never decodes images. The host probes headers for intrinsic size, passes those in, and decodes pixels on its own side of the wall when it paints.

**Cancellation.** A visitor typing in a stylesheet field produces a layout request per keystroke, and all but the last are waste. The protocol carries a generation token so a stale result can be dropped rather than painted.
