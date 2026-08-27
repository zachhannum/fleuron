/**
 * The worker, in the six lines it takes: load the module, hand every
 * message to the engine, post what it says back.
 *
 * The handler goes on before the module has arrived, and every
 * request waits on the same promise. A worker that installs its
 * handler after the load loses whatever a host posted while the
 * module was still coming down, and a host that mounts a preview and
 * hands it a manuscript posts exactly then.
 *
 * A host that wants the module somewhere else, in an extension or a
 * Node thread or a bundle that inlines the bytes, writes these lines
 * itself over {@link createEngine}.
 */

import { createEngine } from './engine.js';
import type { Request, Response } from './protocol.js';

declare const self: {
  onmessage: ((event: { data: Request }) => void) | null;
  postMessage(message: Response, transfer: ArrayBuffer[]): void;
};

const engine = createEngine();

self.onmessage = ({ data }) => {
  // Requests reach the engine in the order they arrived: promise
  // callbacks run in the order they were registered, which is the
  // order the messages did.
  void engine.then((ready) =>
    ready.submit(data, (response, transfer) => self.postMessage(response, transfer)),
  );
};
