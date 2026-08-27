/**
 * The worker the demos run the engine in.
 *
 * The module and the DOM never meet: everything in this file is a
 * message and a byte buffer, and nothing on the other side of the
 * wall has ever heard of an element.
 *
 * The package ships these same six lines. The site writes its own so
 * that the module is fetched from where the site serves it, under the
 * base path and beside the pages, rather than from wherever a bundler
 * decided to put the glue.
 */

import { createEngine } from '@fleuron/wasm';
import type { Request, Response } from '@fleuron/wasm';

interface Scope {
  onmessage: ((event: MessageEvent<Request>) => void) | null;
  postMessage(message: Response, transfer: ArrayBuffer[]): void;
  location: { href: string };
}

const scope = globalThis as unknown as Scope;

// Started before the first message rather than on it: a host that
// mounts a preview and hands it a manuscript posts immediately, and
// the handler below has to be able to take that.
const engine = createEngine({
  wasm: new URL(`${import.meta.env.BASE_URL}fleuron_bg.wasm`, scope.location.href),
});

scope.onmessage = ({ data }) => {
  void engine.then((ready) =>
    ready.submit(data, (response, transfer) => scope.postMessage(response, transfer)),
  );
};
