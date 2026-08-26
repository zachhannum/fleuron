import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

// Glass is where contrast goes to die: text sits on a translucent panel
// over a gradient over the ground, and eyeballing the result is how a
// site ends up at 92. Every pair below is composited through the layers
// actually beneath it and measured.

const AA = 4.5;
const AA_LARGE = 3;

const css = await readFile(
  fileURLToPath(new URL('../src/styles/theme.css', import.meta.url)),
  'utf8',
);

const dark = readTokens(css, ':root {');
const light = readTokens(css, ":root[data-theme='light'] {");

/** Layer stacks, painted back to front, with the text colour last. */
const pairs = [
  ['body text', ['ground'], 'text', AA],
  ['body prose', ['ground'], 'text-muted', AA],
  ['captions and labels', ['ground'], 'text-faint', AA],
  ['text on a panel', ['glass-solid'], 'text', AA],
  ['prose on a panel', ['glass-solid'], 'text-muted', AA],
  ['links', ['ground'], 'accent', AA],
  ['links on a panel', ['glass-solid'], 'accent', AA],
  ['the mark', ['ground'], 'ember', AA_LARGE],
  ['primary button', ['accent-deep'], '#ffffff', AA],
  ['the page', ['paper'], 'paper-ink', AA],
  // The fixed nav: its own 72% ground over the brightest thing the
  // bleeds put behind it, which is the accent bleed at full strength.
  ['nav over the bleed', ['ground', 'accent-deep@0.42', 'ground@0.72'], 'text-muted', AA],
  ['nav title over the bleed', ['ground', 'accent-deep@0.42', 'ground@0.72'], 'text', AA],
];

let failed = 0;
for (const [theme, tokens] of [
  ['dark', dark],
  ['light', light],
]) {
  for (const [what, stack, fg, floor] of pairs) {
    const bg = composite(stack.map((layer) => resolve(layer, tokens)));
    const ratio = contrast(resolve(fg, tokens), bg);
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

/** `name`, `name@alpha` or a literal `#rrggbb`, as `[r, g, b, a]`. */
function resolve(spec, tokens) {
  const [name, alpha] = spec.split('@');
  const raw = name.startsWith('#') ? name : tokens.get(name);
  if (!raw) throw new Error(`unknown token --f-${name}`);
  const colour = parse(raw);
  return alpha ? [colour[0], colour[1], colour[2], Number(alpha)] : colour;
}

function parse(value) {
  const hex = value.match(/^#([0-9a-f]{6})$/i);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  const rgb = value.match(
    /^rgb\(\s*(\d+)\s+(\d+)\s+(\d+)\s*(?:\/\s*([\d.]+)\s*)?\)$/,
  );
  if (rgb) {
    return [+rgb[1], +rgb[2], +rgb[3], rgb[4] === undefined ? 1 : +rgb[4]];
  }
  throw new Error(`cannot measure ${value}`);
}

/** Source-over, back to front. The first layer is taken as opaque. */
function composite(layers) {
  let out = [layers[0][0], layers[0][1], layers[0][2]];
  for (const [r, g, b, a] of layers.slice(1)) {
    out = [
      r * a + out[0] * (1 - a),
      g * a + out[1] * (1 - a),
      b * a + out[2] * (1 - a),
    ];
  }
  return [...out, 1];
}

function contrast(fg, bg) {
  const over = composite([bg, fg]);
  const a = luminance(over);
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
