/**
 * The worker, in the eight lines it takes: load the module, hand
 * every message to the engine, post what it says back.
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

const engine = await createEngine();

self.onmessage = ({ data }) => {
  engine.submit(data, (response, transfer) => self.postMessage(response, transfer));
};
