/**
 * The poster for a demo: a page the engine painted at build time.
 *
 * Generated rather than checked in, from the same inputs the island
 * runs, so a poster cannot disagree with the engine it stands in
 * for. `npm run demos` writes them.
 */

const painted = import.meta.glob('../generated/posters/*.svg', {
  query: '?raw',
  import: 'default',
  eager: true,
});

/** One demo's poster, as SVG markup. */
export function poster(id) {
  const found = painted[`../generated/posters/${id}.svg`];
  if (found === undefined) {
    throw new Error(`no poster for ${id}: run npm run demos`);
  }
  return found;
}
