/**
 * A static server over the repository, so the harness can be opened.
 *
 * The package is served from where it is built rather than from
 * `node_modules`, and the fixture book from `fixtures/`, which is why
 * the root is the repository and not this directory.
 */

import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));

const types = {
  '.css': 'text/css',
  '.html': 'text/html',
  '.jpg': 'image/jpeg',
  '.js': 'text/javascript',
  '.json': 'application/json',
  '.md': 'text/markdown',
  '.png': 'image/png',
  '.pdf': 'application/pdf',
  '.svg': 'image/svg+xml',
  '.ttf': 'font/ttf',
  '.wasm': 'application/wasm',
};

/** Serves the repository, and refuses to serve anything outside it. */
export function serve(port = 0) {
  const server = createServer(async (request, response) => {
    const path = join(root, normalize(decodeURIComponent(new URL(request.url ?? '/', 'http://x').pathname)));
    if (!path.startsWith(root)) {
      response.writeHead(403).end();
      return;
    }
    try {
      const found = (await stat(path)).isDirectory() ? join(path, 'index.html') : path;
      await stat(found);
      response.writeHead(200, {
        'content-type': types[extname(found)] ?? 'application/octet-stream',
        'cache-control': 'no-store',
      });
      createReadStream(found)
        .on('error', () => response.end())
        .pipe(response);
    } catch {
      response.writeHead(404).end();
    }
  });
  return new Promise((resolve) => {
    server.listen(port, '127.0.0.1', () => resolve(server));
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const server = await serve(Number(process.env['PORT'] ?? 8080));
  const { port } = server.address();
  console.log(`the harness is at http://127.0.0.1:${port}/examples/preview/`);
}
