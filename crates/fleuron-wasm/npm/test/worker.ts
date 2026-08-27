/**
 * The harness's worker: the same eight lines the package ships as
 * its browser worker, over Node's threads and with the module's
 * bytes read from disk, since there is nothing here to fetch them
 * from.
 */

import { readFileSync } from 'node:fs';
import { parentPort, workerData } from 'node:worker_threads';

import { createEngine, type Request } from '../dist/index.js';

if (parentPort === null) {
  throw new Error('this module is a worker, and was loaded as one');
}

const port = parentPort;
const engine = await createEngine({ wasm: readFileSync((workerData as { wasm: string }).wasm) });

port.on('message', (request: Request) => {
  engine.submit(request, (response, transfer) => port.postMessage(response, transfer));
});
