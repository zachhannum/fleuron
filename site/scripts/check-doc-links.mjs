/**
 * Every relative link between prose files, resolved against the
 * files.
 *
 * Prose in `docs/` is read on GitHub as well as here, and there a
 * link is a path on disk rather than a route. The site's own link
 * validator checks the routes; this checks the paths, which is what
 * a reader who never visits the site follows.
 */

import { readFile, readdir, stat } from 'node:fs/promises';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const docs = fileURLToPath(new URL('../../docs/', import.meta.url));
const root = fileURLToPath(new URL('../../', import.meta.url));

async function prose(dir) {
  const found = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await prose(path)));
    } else if (/\.mdx?$/.test(entry.name)) {
      found.push(path);
    }
  }
  return found;
}

/** Prose with the code taken out: `[text](url)` in a table is a spelling, not a link. */
function prune(text) {
  return text.replace(/```[\s\S]*?```/g, '').replace(/`[^`\n]*`/g, '');
}

let broken = 0;
let checked = 0;

for (const file of await prose(docs)) {
  const text = prune(await readFile(file, 'utf8'));
  for (const [, label, url] of text.matchAll(/\[([^\]]*)\]\(([^)\s]+)\)/g)) {
    if (/^[a-z]+:/i.test(url) || url.startsWith('#') || url.startsWith('/')) {
      continue;
    }
    const target = resolve(dirname(file), url.split('#')[0] ?? '');
    checked += 1;
    try {
      await stat(target);
    } catch {
      broken += 1;
      console.error(
        `${relative(root, file)}: [${label}](${url}) points at nothing`,
      );
    }
  }
}

if (broken > 0) {
  console.error(`${broken} of ${checked} prose links point at nothing`);
  process.exit(1);
}
console.log(`${checked} prose links resolve to the files they name`);
