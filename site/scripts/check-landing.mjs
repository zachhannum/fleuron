import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// The landing page is typography, CSS and one SVG. Nothing on it has a
// reason to hydrate, and a stray integration that starts injecting a
// script should fail the build rather than the Lighthouse run.
const page = fileURLToPath(new URL('../dist/index.html', import.meta.url));
const html = await readFile(page, 'utf8');
const scripts = html.match(/<script\b[^>]*>/g) ?? [];

if (scripts.length > 0) {
  console.error(`landing page ships ${scripts.length} script tag(s):`);
  for (const tag of scripts) console.error(`  ${tag}`);
  process.exit(1);
}

console.log('landing page ships zero JavaScript');
