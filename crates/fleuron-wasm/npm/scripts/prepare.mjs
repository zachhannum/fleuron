// wasm-pack writes a `.gitignore` of `*` into its output directory,
// on the assumption that nothing there is worth keeping. Here the
// module is the package, and npm reads a nested `.gitignore` as an
// instruction to leave the directory out of the tarball. So it goes.

import { rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

rmSync(fileURLToPath(new URL('../wasm/.gitignore', import.meta.url)), { force: true });
