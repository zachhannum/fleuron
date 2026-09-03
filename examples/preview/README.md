# The preview harness

A page that opens the fixture book, pages through it, and puts the
browser's own PDF viewer beside the preview, over the same run.

```sh
cargo install wasm-pack
wasm-pack build crates/fleuron-wasm --target web --release --no-pack \
  --out-dir npm/wasm --out-name fleuron
cd crates/fleuron-wasm/npm && npm ci && npm run build

node examples/preview/serve.mjs
```

The server serves the repository, since the package is served from
where it is built rather than from `node_modules`, and the fixture book
from `fixtures/`.

Drop any markdown file on the manuscript input, and any stylesheet on
the other one.

The fixture book's images are fetched by the page and handed to the
preview as bytes, which is what a host does: nothing in the package
fetches a url.

`harness.js` is written the way a consumer of `@fleuron/wasm` writes
one: it names no buffer, no worker and no display structure. That is why the
browser run in `crates/fleuron-wasm/npm/test/browser.ts` drives this
page rather than one of its own.
