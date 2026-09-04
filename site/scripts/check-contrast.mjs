import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// Four process inks and two grounds. Nothing on this site is
// translucent, so every pair below is a colour on a colour: what is
// measured is what a reader sees.

const AA = 4.5;
const AA_LARGE = 3;

const css = await readFile(
  fileURLToPath(new URL('../src/styles/theme.css', import.meta.url)),
  'utf8',
);

// Light is the designed theme and dark is a swap of nine of its
// tokens, so the dark palette is the light one with that block
// applied over it.
const light = readTokens(css, ':root {');
const dark = new Map([...light, ...readTokens(css, ":root[data-theme='dark'] {")]);

/** Background, then the text colour on it. */
const pairs = [
  ['body text', 'ground', 'ink', AA],
  ['chrome text', 'ground', 'ink-2', AA],
  ['text on a panel', 'panel', 'panel-ink', AA],
  ['the eyebrow', 'hi', 'hi-ink', AA],
  ['the highlight', 'hi', 'hi-ink', AA],
  ['a rule as a ground', 'rule', 'ground', AA],
  ['the primary button', 'btn-p-bg', 'btn-p-ink', AA],
  ['the performance band', 'bright-bg', 'bright-ink', AA],
  ['the footer', 'foot-bg', 'foot-ink', AA],
  ['code', 'code-bg', 'code-ink', AA],
  ['the editor', 'edit-bg', 'edit-ink', AA],
  ['the live tab', 'hi', 'hi-ink', AA],
  ['the pasteboard', 'board', 'board-ink', AA],
  ['the page', '#ffffff', '#14151a', AA],
  // Yellow on key: the settings links in the editor half.
  ['yellow on the editor', 'edit-bg', 'ink-y', AA],
  // The mark, which is a glyph rather than a word.
  ['the mark', 'ground', 'ink-m', AA_LARGE],
];

let failed = 0;
for (const [theme, tokens] of [
  ['light', light],
  ['dark', dark],
]) {
  for (const [what, back, fore, floor] of pairs) {
    const ratio = contrast(resolve(fore, tokens), resolve(back, tokens));
    const ok = ratio >= floor;
    if (!ok) failed++;
    console.log(
      `${ok ? 'ok  ' : 'FAIL'} ${theme.padEnd(5)} ${what.padEnd(24)} ${ratio.toFixed(2)}:1 (needs ${floor})`,
    );
  }
}

if (failed > 0) {
  console.error(`\n${failed} contrast pair(s) below AA`);
  process.exit(1);
}

function readTokens(source, opener) {
  const start = source.indexOf(opener);
  if (start === -1) throw new Error(`no ${opener} block in theme.css`);
  const block = source.slice(start, source.indexOf('}', start));
  const tokens = new Map();
  for (const [, name, value] of block.matchAll(/--f-([\w-]+):\s*([^;]+);/g)) {
    tokens.set(name, value.trim());
  }
  return tokens;
}

/** A token name or a literal `#rrggbb`, as `[r, g, b]`. */
function resolve(spec, tokens) {
  if (spec.startsWith('#')) return parse(spec);
  let value = tokens.get(spec);
  if (value === undefined) throw new Error(`unknown token --f-${spec}`);
  // A token may point at another one; the inks are named that way.
  const seen = new Set();
  let alias = value.match(/^var\(--f-([\w-]+)\)$/);
  while (alias !== null) {
    if (seen.has(alias[1])) throw new Error(`--f-${spec} points at itself`);
    seen.add(alias[1]);
    value = tokens.get(alias[1]);
    if (value === undefined) throw new Error(`unknown token --f-${alias[1]}`);
    alias = value.match(/^var\(--f-([\w-]+)\)$/);
  }
  return parse(value);
}

function parse(value) {
  const hex = value.match(/^#([0-9a-f]{6})$/i);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }
  throw new Error(`cannot measure ${value}`);
}

function contrast(fg, bg) {
  const a = luminance(fg);
  const b = luminance(bg);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

function luminance([r, g, b]) {
  const [rl, gl, bl] = [r, g, b].map((channel) => {
    const c = channel / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * rl + 0.7152 * gl + 0.0722 * bl;
}
