/**
 * The preview painter: one page of the display structure, as SVG.
 *
 * The engine has already shaped and broken the text, so the browser
 * is given no room to do either. Every run is one `<text>` carrying
 * an x for each character it holds, taken from the glyph the shaper
 * put there, and the face is the file the engine shaped with, pinned
 * to the instance it shaped at. A preview that disagrees with the
 * export about where a glyph sits therefore has a bug here, and
 * nowhere else.
 *
 * The coordinate system is the display structure's: points, origin top
 * left, on a `viewBox` the size of the trim. Zoom is the width and
 * height the element is given, and moves nothing inside it.
 */

import type { Asset, DrawItem, FontRefEntry, ImageItem, Page, RectItem, TextItem } from './wire.js';

/** How a page is painted. */
export interface PaintOptions {
  /** The run's font table: `LayoutOutput.fonts`. */
  fonts?: FontRefEntry[];
  /** The run's asset table: `LayoutOutput.assets`. */
  assets?: Asset[];
  /** Points to CSS pixels. 1 draws the page at its trim size. */
  zoom?: number;
  /** What the page is printed on; `null` leaves it transparent. */
  paper?: string | null;
  /** What it is printed in. */
  ink?: string;
  /**
   * Where an image's pixels come from: any url an `<img>` would
   * take, including a `blob:` or `data:` one, or nothing when the
   * host cannot supply them.
   *
   * Called with the asset the display structure placed, so a host keyed
   * on urls needs no index of its own. It is only ever called when
   * {@link PaintOptions.assets} is given, since the index means
   * nothing without the table it indexes.
   */
  asset?: (asset: Asset, index: number) => string | null | undefined;
}

/**
 * What a face is registered as, so a painter and whoever loaded the
 * bytes agree on the name.
 *
 * Per id rather than per family: a variable file names several cuts,
 * and each is drawn at its own place on the axes.
 */
export function faceFamily(fontId: number): string {
  return `fleuron-face-${fontId}`;
}

/**
 * Paints one page, as an `<svg>` element's markup.
 *
 * A face that is not loaded falls through the stack to whatever the
 * browser has, so a page whose fonts never arrived is set in the
 * wrong face rather than left blank.
 */
export function paintPage(page: Page, options: PaintOptions = {}): string {
  const zoom = options.zoom ?? 1;
  const paper = options.paper === undefined ? '#ffffff' : options.paper;
  const body = page.items.map((item) => paint(item, options)).join('');
  const ground =
    paper === null
      ? ''
      : `<rect x="0" y="0" width="${num(page.width)}" height="${num(page.height)}" fill="${escape(paper)}"/>`;
  return (
    `<svg xmlns="http://www.w3.org/2000/svg"` +
    ` viewBox="0 0 ${num(page.width)} ${num(page.height)}"` +
    ` width="${num(page.width * zoom)}" height="${num(page.height * zoom)}"` +
    ` fill="${escape(options.ink ?? '#000000')}"` +
    ` data-page="${page.number}" data-side="${page.side}">` +
    `${ground}${body}</svg>`
  );
}

function paint(item: DrawItem, options: PaintOptions): string {
  switch (item.kind) {
    case 'text':
      return text(item, options);
    case 'rect':
      return rect(item);
    case 'image':
      return image(item, options);
  }
}

function text(item: TextItem, options: PaintOptions): string {
  const entry = options.fonts?.[item.fontId];
  const xs = positions(item);
  // Slope and weight are the face's own rather than the container's.
  // A host element that happens to be bold would otherwise have the
  // browser synthesise a second bold over a face that is already
  // one, and a face that did not arrive would fall back upright.
  return (
    `<text x="${xs.map(num).join(' ')}" y="${num(item.y)}"` +
    ` font-family="${escape(stack(item.fontId, entry))}"` +
    ` font-size="${num(item.size)}"` +
    ` font-weight="${entry?.attributes.weight ?? 400}"` +
    ` font-style="${entry?.attributes.italic === true ? 'italic' : 'normal'}"` +
    ` style="${escape(style(entry))}" xml:space="preserve"` +
    (entry === undefined ? ` data-missing-font="${item.fontId}"` : '') +
    `>${escape(item.text)}</text>`
  );
}

function rect(item: RectItem): string {
  return `<rect x="${num(item.x)}" y="${num(item.y)}" width="${num(item.w)}" height="${num(item.h)}"/>`;
}

/**
 * A placed image. The pixels are the host's, and one it cannot
 * supply is drawn as the box layout reserved for it rather than as
 * nothing.
 */
function image(item: ImageItem, options: PaintOptions): string {
  const box =
    `x="${num(item.x)}" y="${num(item.y)}"` +
    ` width="${num(item.w)}" height="${num(item.h)}"`;
  const asset = options.assets?.[item.asset];
  const href = asset === undefined ? undefined : options.asset?.(asset, item.asset);
  return href === null || href === undefined
    ? `<rect ${box} fill="none" stroke="currentColor" stroke-dasharray="3 3" data-missing-asset="${item.asset}"/>`
    : `<image ${box} href="${escape(href)}" preserveAspectRatio="none"/>`;
}

/**
 * The families a run is drawn in, best first: the file the engine
 * shaped with, then whatever the reader has under the same name,
 * then a generic.
 */
function stack(fontId: number, entry: FontRefEntry | undefined): string {
  const names = [faceFamily(fontId), ...(entry === undefined ? [] : [entry.family])];
  return [...names.map((name) => `"${name.replace(/["\\]/g, '')}"`), 'serif'].join(', ');
}

/**
 * Where on its axes the face is drawn. A cut the file named is a
 * location on that file, not a file of its own, so a painter that
 * does not pin it draws the default weight for every one of them.
 */
function style(entry: FontRefEntry | undefined): string {
  const settings = (entry?.variations ?? [])
    .map((axis) => `"${axis.tag}" ${num(axis.value)}`)
    .join(', ');
  // Runs carry the spaces the line was justified around, and SVG
  // collapses them by default, which would slide every character
  // after the first space one position along the x list.
  return `white-space: pre` + (settings === '' ? '' : `; font-variation-settings: ${settings}`);
}

/**
 * An x for each character of a run, from the glyph the shaper placed
 * there.
 *
 * SVG positions characters, and the display structure positions glyphs,
 * so the run's text is the join between them: each glyph carries the
 * byte range it stands for, and its x lands on the character that
 * range starts at. A character inside a ligature's range has no
 * glyph of its own and is spaced evenly across it, which is what the
 * browser does with it in the one case it matters: when the face that
 * arrived forms no ligature and draws the characters separately.
 */
function positions(item: TextItem): number[] {
  const starts = characters(item.text);
  const index = new Map(starts.map((byte, at) => [byte, at]));
  const xs: (number | undefined)[] = new Array<number | undefined>(starts.length);
  for (const glyph of item.glyphs) {
    const at = index.get(glyph.range[0]);
    if (at === undefined) {
      continue;
    }
    const held = xs[at];
    if (held === undefined || glyph.x < held) {
      xs[at] = glyph.x;
    }
  }
  return fill(xs, item.x);
}

/** The byte offset each character of a string begins at. */
function characters(text: string): number[] {
  const starts: number[] = [];
  let byte = 0;
  for (const character of text) {
    starts.push(byte);
    const code = character.codePointAt(0) ?? 0;
    byte += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
  }
  return starts;
}

/**
 * Closes the gaps a glyph-to-character mapping leaves: interior ones
 * are spaced evenly, and a run of them at the end is dropped, since
 * an x list may be shorter than the text and the browser advances
 * the rest itself.
 */
function fill(xs: (number | undefined)[], left: number): number[] {
  // Where the next placed character is, read off in one pass
  // backwards, so a long gap costs no more than a short one.
  const ahead: number[] = new Array<number>(xs.length);
  let next = -1;
  let last = -1;
  for (let at = xs.length - 1; at >= 0; at -= 1) {
    ahead[at] = next;
    if (xs[at] !== undefined) {
      next = at;
      if (last === -1) {
        last = at;
      }
    }
  }
  if (last === -1) {
    return [left];
  }
  const out: number[] = [];
  let held = left;
  for (let at = 0; at <= last; at += 1) {
    const x = xs[at];
    const to = ahead[at] ?? -1;
    held = x ?? held + ((xs[to] ?? held) - held) / (to - at + 1);
    out.push(held);
  }
  return out;
}

/**
 * The shortest decimal that reads back as the same 32-bit float.
 *
 * The display structure is `f32`, and printing one as the double it
 * widened to writes seventeen digits of a number that only has
 * seven. Both are the same position; only one of them is worth
 * sending a page's worth of.
 */
function num(value: number): string {
  for (let digits = 0; digits <= 8; digits += 1) {
    const text = value.toFixed(digits);
    if (Math.fround(Number(text)) === Math.fround(value)) {
      return digits === 0 ? text : text.replace(/\.?0+$/, '');
    }
  }
  return String(value);
}

function escape(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}
