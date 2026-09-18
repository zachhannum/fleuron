/**
 * The wire: postcard bytes in, a display structure out.
 *
 * The engine encodes with postcard, which sends no field names and
 * packs small numbers into one byte. Nothing in the buffer says what
 * it is, so this reader walks the same fields in the same order the
 * engine wrote them, and the version in front of the bytes is what
 * catches the day those two stop agreeing.
 */

import type { PageBox } from './protocol.js';

/** The encoding this reader reads. */
export const WIRE_VERSION = 15;

/**
 * The layer the background of a page paints in: under every layer a
 * stylesheet can name. Where a stylesheet names no `z-index`, the
 * text of a page still covers the background.
 */
export const PAGE_BACKGROUND = -2147483648;

/**
 * The layer a page number and a running head paint in: over every
 * layer a stylesheet can name.
 */
export const PAGE_FURNITURE = 2147483647;

/** Which side of the spread a page falls on. */
export type Side = 'recto' | 'verso';

/** The features beyond the default set a run was shaped with. */
export interface Features {
  /** `smcp`: the face's own small capitals. */
  smallCaps: boolean;
}

/** One glyph: an id in its font, an absolute x, and the text it stands for. */
export interface Glyph {
  /** Glyph id in the run's font. */
  id: number;
  /** Absolute x of the glyph's origin, in points. */
  x: number;
  /** Byte range in the run's text this glyph came from. */
  range: [number, number];
}

/** Where a run was written, in the content tree it came from. */
export interface SourceRange {
  /** The id of the node the text was written in. */
  node: number;
  /** Byte range in that node's own text. */
  range: [number, number];
}

/** A run of shaped glyphs sharing a font, a size and a baseline. */
export interface TextItem {
  kind: 'text';
  /** Left edge of the run. */
  x: number;
  /** The run's baseline. */
  y: number;
  /** Index into {@link LayoutOutput.fonts}. */
  fontId: number;
  /** Em size in points. */
  size: number;
  /**
   * The text the glyphs were shaped from, which each glyph's range
   * indexes. A painter that draws characters rather than glyphs
   * draws these.
   */
  text: string;
  /**
   * What the author wrote, where `text-transform` or small capitals
   * made that differ from what was shaped, and empty where the two
   * are the same. Selection, copy-and-paste and accessible text read
   * this rather than {@link TextItem.text}.
   */
  source: string;
  /**
   * The offset in {@link TextItem.source} of every byte boundary of
   * {@link TextItem.text}, so a glyph's range taken through it is
   * the source that glyph stands for. Empty alongside `source`.
   */
  sourceMap: number[];
  /**
   * Where the run was written: the content node it was shaped from
   * and the bytes of that node's own text it stands for. The runs
   * naming one node tile it, so a cursor in the manuscript lands on
   * a run and a run lands back on the manuscript. Null on text the
   * engine wrote itself: a folio, a running head, a scene break's
   * ornament.
   */
  origin: SourceRange | null;
  /**
   * The id of the pseudo-element the run was cut from: a drop cap,
   * the line a paragraph opens on, or the text of `::before` or
   * `::after`. `origin` still names the text it stands for. Null on
   * every other run.
   */
  pseudoElement: number | null;
  /**
   * The OpenType features the run was shaped with. A painter that
   * draws characters asks the face for these, or the browser picks
   * its own glyphs and sets them at positions measured for others.
   */
  features: Features;
  /**
   * The `#rrggbb` the run is painted in, or `#rrggbbaa` where the
   * colour has an alpha. A sheet that names no colour leaves it black.
   */
  color: string;
  /** The glyphs, in visual order. */
  glyphs: Glyph[];
  /** Which layer the run paints in. */
  layer: number;
}

/** A filled rectangle: rules, borders, backgrounds. */
export interface RectItem {
  kind: 'rect';
  /** Left edge. */
  x: number;
  /** Top edge. */
  y: number;
  /** Width in points. */
  w: number;
  /** Height in points. */
  h: number;
  /** The `#rrggbb` or `#rrggbbaa` the rectangle is filled with. */
  color: string;
  /** Which layer the rectangle paints in. */
  layer: number;
}

/** An image's own idea of its size, from its header. */
export interface Intrinsic {
  /** Width in pixels. */
  width: number;
  /** Height in pixels. */
  height: number;
  /** Horizontal resolution in pixels per inch. */
  dpiX: number;
  /** Vertical resolution in pixels per inch. */
  dpiY: number;
}

/**
 * One image the book placed, as {@link ImageItem.asset} indexes it.
 *
 * All the display structure says about an image: layout read the header
 * and decoded nothing, so a painter takes the url back to its own
 * pixels.
 */
export interface Asset {
  /** The url the content tree named it by. */
  url: string;
  /** What the header says it is. */
  intrinsic: Intrinsic;
}

/** A placed image. Layout never decoded it; the painter does. */
export interface ImageItem {
  kind: 'image';
  /** Left edge. */
  x: number;
  /** Top edge. */
  y: number;
  /** Width in points. */
  w: number;
  /** Height in points. */
  h: number;
  /** Index into {@link LayoutOutput.assets}. */
  asset: number;
  /**
   * How much of the image shows, from 0 to 255, where 255 is all of
   * it: `opacity` on the image and the blocks around it.
   */
  alpha: number;
  /** Which layer the image paints in. */
  layer: number;
}

/**
 * An image painted behind a box: the page's own, or a block's border
 * box.
 *
 * The box is what the image is clipped to, and the tile is where one
 * copy of it is drawn, which may reach outside the box. A painter
 * clips to the box, draws the tile, and repeats the tile across and
 * down the box where {@link BackgroundItem.repeat} asks for it.
 */
export interface BackgroundItem {
  kind: 'background';
  /** Left edge of the box the image is painted behind. */
  x: number;
  /** Its top edge. */
  y: number;
  /** Its width in points. */
  w: number;
  /** Its height in points. */
  h: number;
  /** How far each corner of the box is rounded. The image is clipped to the rounded box. */
  radii: Corners;
  /** Left edge of the first tile. */
  tileX: number;
  /** Its top edge. */
  tileY: number;
  /** The width one copy of the image is drawn at. */
  tileW: number;
  /** The height one copy is drawn at. */
  tileH: number;
  /** Whether the tile repeats to cover the box. */
  repeat: boolean;
  /** Index into {@link LayoutOutput.assets}. */
  asset: number;
  /**
   * How much of the image shows, from 0 to 255, where 255 is all of
   * it: `opacity` on the blocks the box belongs to.
   */
  alpha: number;
  /** Which layer the image paints in. */
  layer: number;
}

/** How far one corner of a box is rounded: the two radii of the quarter ellipse it follows. */
export interface Radius {
  /** Along the top or bottom edge, in points. */
  x: number;
  /** Along the left or right edge, in points. */
  y: number;
}

/**
 * The four corners of a box, each rounded by its own radius. A corner
 * with a radius of zero on either axis is square. The radii of two
 * corners on one edge never add up to more than the edge is long.
 */
export interface Corners {
  topLeft: Radius;
  topRight: Radius;
  bottomRight: Radius;
  bottomLeft: Radius;
}

/** One number for each edge of a box, in points. */
export interface Edges {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

/**
 * A filled box with rounded corners: a background, or a border drawn
 * as a ring.
 *
 * The outer shape is the box with its corners rounded by
 * {@link RoundedItem.radii}. Where {@link RoundedItem.ring} is zero on
 * all four edges, the whole shape is filled. Otherwise the fill is the
 * band between the outer shape and an inner one: the box `ring` in
 * from each edge, where each corner radius loses the width of the edge
 * it runs along and goes no lower than zero.
 */
export interface RoundedItem {
  kind: 'rounded';
  /** Left edge. */
  x: number;
  /** Top edge. */
  y: number;
  /** Width in points. */
  w: number;
  /** Height in points. */
  h: number;
  /** How far each corner is rounded. */
  radii: Corners;
  /** How far in from each edge the fill reaches. Zero on all four fills the whole shape. */
  ring: Edges;
  /** The `#rrggbb` or `#rrggbbaa` the shape is filled with. */
  color: string;
  /** Which layer the shape paints in. */
  layer: number;
}

/** A single paint operation. */
export type DrawItem = TextItem | RectItem | ImageItem | BackgroundItem | RoundedItem;

/** One typeset page, and what to paint on it. */
export interface Page {
  /** Folio, counting from 1. */
  number: number;
  /** Which side of the spread this page falls on. */
  side: Side;
  /** Trimmed page width in points. */
  width: number;
  /** Trimmed page height in points. */
  height: number;
  /**
   * The content-tree node ids of the sections whose content appears on
   * this page, in the order their content appears on it. A chapter that
   * ends mid-page is followed there by the next one opening, so the
   * page names both. A blank leaf names none.
   */
  sections: number[];
  /**
   * What to paint, in paint order: by layer, and inside one layer in
   * the order the blocks are written.
   */
  items: DrawItem[];
  /**
   * The links set on this page, in the order their text is painted.
   * Empty on a page with no link. {@link linkAt} finds the one under a
   * point.
   */
  links: Link[];
}

/** One link on one page: the area its text covers on each line, and where it goes. */
export interface Link {
  /**
   * One area for each line the link is set on in this page. Each runs
   * across the link's glyphs on that line, the text of its `::before`
   * and `::after` included, and down from the ascent to the descent of
   * the face. The space between two lines is in none of them.
   */
  areas: PageBox[];
  /** Where the link goes. */
  to: LinkTo;
}

/**
 * Where a link goes: a place in the book, or a url outside it.
 *
 * A place carries everything a host needs to follow it without the
 * page it lands on: `place.page` is the place of that page in the
 * book, counting from 0, as `Request.first` counts.
 */
export type LinkTo =
  | {
      kind: 'place';
      /** The id of the element the link names. */
      node: number;
      /** The box of that element, on the page it opens on. */
      place: PageBox;
    }
  | {
      kind: 'uri';
      /** The url, as the manuscript wrote it. */
      url: string;
    };

/** One axis of a variable face, pinned. */
export interface AxisSetting {
  /** The four-character OpenType axis tag, e.g. `wght`. */
  tag: string;
  /** The coordinate in the axis's own units. */
  value: number;
}

/** The slope and weight a face answers for. */
export interface FaceAttributes {
  /** True for italic and oblique alike. */
  italic: boolean;
  /** Weight on the CSS 1–1000 scale. */
  weight: number;
}

/** A font's identity in the display structure. */
export interface FontRefEntry {
  /** Family for matching, lowercase. */
  family: string;
  /** Face name. */
  name: string;
  /** Style name. */
  style: string;
  /** What this face answers for. */
  attributes: FaceAttributes;
  /**
   * Where on its file's axes this face sits, in user space. Empty
   * for a static face and for a variable one at its default
   * location. A painter pins these axes and draws the cut the run
   * was shaped at rather than the file's default.
   */
  variations: AxisSetting[];
}

/** A book that laid out anyway, and what it had to complain about. */
export interface Warning {
  /** What went wrong, in one line. */
  message: string;
  /** Where it was written, when a position was recorded. */
  origin: string | null;
}

/**
 * What one reply carried, and where it falls in the book.
 *
 * `pages` need not be the whole book: a request that named a range
 * gets back that slice alone, `first` says where it begins (counting
 * from 0), and `bookPages` is how many pages the book has, so a reply
 * carrying page 12 alone still answers "page 12 of 337". `fonts`,
 * `assets` and `warnings` are never sliced, since none of them is per
 * page.
 */
export interface LayoutOutput {
  /** The pages this reply carries, in reading order. */
  pages: Page[];
  /** The index of `pages[0]` in the book. Zero for a whole-book reply. */
  first: number;
  /** How many pages the book has. */
  bookPages: number;
  /** The fonts this run used, indexed by `fontId`. */
  fonts: FontRefEntry[];
  /** The images this run placed, indexed by `ImageItem.asset`. */
  assets: Asset[];
  /** Everything the run had to complain about. */
  warnings: Warning[];
}

/** A buffer this reader will not read, and why. */
export class WireError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'WireError';
  }
}

/** Walks a postcard buffer, one field at a time. */
class Reader {
  private readonly view: DataView;
  private readonly bytes: Uint8Array;
  private at = 0;

  constructor(bytes: Uint8Array) {
    this.bytes = bytes;
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  /** An unsigned varint, seven bits per byte, low group first. */
  varint(): number {
    let value = 0;
    let shift = 0;
    for (;;) {
      const byte = this.bytes[this.at++];
      if (byte === undefined) {
        throw new WireError('the buffer ended mid-number');
      }
      value += (byte & 0x7f) * 2 ** shift;
      if ((byte & 0x80) === 0) {
        return value;
      }
      shift += 7;
      if (shift > 63) {
        throw new WireError('a varint ran past the width of a number');
      }
    }
  }

  /** A signed varint: zigzag encoded, so the sign is the low bit. */
  signed(): number {
    const value = this.varint();
    return value % 2 === 0 ? value / 2 : -(value + 1) / 2;
  }

  bool(): boolean {
    return this.varint() !== 0;
  }

  f32(): number {
    const value = this.view.getFloat32(this.at, true);
    this.at += 4;
    return value;
  }

  string(): string {
    const length = this.varint();
    const start = this.at;
    this.at += length;
    if (this.at > this.bytes.length) {
      throw new WireError('the buffer ended mid-string');
    }
    return decoder.decode(this.bytes.subarray(start, this.at));
  }

  /** A `Vec<T>`: a count, then that many of them. */
  seq<T>(item: () => T): T[] {
    const length = this.varint();
    const out: T[] = new Array<T>(length);
    for (let i = 0; i < length; i += 1) {
      out[i] = item();
    }
    return out;
  }

  /** One byte. A `u8` is written as itself, not as a varint. */
  byte(): number {
    const value = this.bytes[this.at++];
    if (value === undefined) {
      throw new WireError('the buffer ended mid-byte');
    }
    return value;
  }

  /**
   * Four bytes, one per channel and one for alpha, read back as
   * `#rrggbb`, or as `#rrggbbaa` where the colour is not opaque.
   */
  color(): string {
    const hex = (channel: number): string => channel.toString(16).padStart(2, '0');
    const digits = [this.byte(), this.byte(), this.byte()].map(hex).join('');
    const alpha = this.byte();
    return alpha === 255 ? `#${digits}` : `#${digits}${hex(alpha)}`;
  }

  /** An `Option<T>`: present or not, and the value when it is. */
  option<T>(item: () => T): T | null {
    return this.varint() === 0 ? null : item();
  }

  done(): boolean {
    return this.at >= this.bytes.length;
  }
}

const decoder = new TextDecoder();

const SIDES: Side[] = ['recto', 'verso'];

function glyph(r: Reader): Glyph {
  return { id: r.varint(), x: r.f32(), range: [r.varint(), r.varint()] };
}

function sourceRange(r: Reader): SourceRange {
  return { node: r.varint(), range: [r.varint(), r.varint()] };
}

function radius(r: Reader): Radius {
  return { x: r.f32(), y: r.f32() };
}

function corners(r: Reader): Corners {
  return { topLeft: radius(r), topRight: radius(r), bottomRight: radius(r), bottomLeft: radius(r) };
}

function edges(r: Reader): Edges {
  return { top: r.f32(), right: r.f32(), bottom: r.f32(), left: r.f32() };
}

function item(r: Reader): DrawItem {
  const variant = r.varint();
  switch (variant) {
    case 0:
      return {
        kind: 'text',
        x: r.f32(),
        y: r.f32(),
        fontId: r.varint(),
        size: r.f32(),
        text: r.string(),
        source: r.string(),
        sourceMap: r.seq(() => r.varint()),
        origin: r.option(() => sourceRange(r)),
        pseudoElement: r.option(() => r.varint()),
        features: { smallCaps: r.bool() },
        color: r.color(),
        glyphs: r.seq(() => glyph(r)),
        layer: r.signed(),
      };
    case 1:
      return {
        kind: 'rect',
        x: r.f32(),
        y: r.f32(),
        w: r.f32(),
        h: r.f32(),
        color: r.color(),
        layer: r.signed(),
      };
    case 2:
      return {
        kind: 'image',
        x: r.f32(),
        y: r.f32(),
        w: r.f32(),
        h: r.f32(),
        asset: r.varint(),
        alpha: r.byte(),
        layer: r.signed(),
      };
    case 3:
      return {
        kind: 'background',
        x: r.f32(),
        y: r.f32(),
        w: r.f32(),
        h: r.f32(),
        radii: corners(r),
        tileX: r.f32(),
        tileY: r.f32(),
        tileW: r.f32(),
        tileH: r.f32(),
        repeat: r.bool(),
        asset: r.varint(),
        alpha: r.byte(),
        layer: r.signed(),
      };
    case 4:
      return {
        kind: 'rounded',
        x: r.f32(),
        y: r.f32(),
        w: r.f32(),
        h: r.f32(),
        radii: corners(r),
        ring: edges(r),
        color: r.color(),
        layer: r.signed(),
      };
    default:
      throw new WireError(`draw item ${variant} is not one this reader reads`);
  }
}

function pageBox(r: Reader): PageBox {
  return { page: r.varint(), x: r.f32(), y: r.f32(), width: r.f32(), height: r.f32() };
}

function linkTo(r: Reader): LinkTo {
  const variant = r.varint();
  switch (variant) {
    case 0:
      return { kind: 'place', node: r.varint(), place: pageBox(r) };
    case 1:
      return { kind: 'uri', url: r.string() };
    default:
      throw new WireError(`link target ${variant} is not one this reader reads`);
  }
}

function link(r: Reader): Link {
  return { areas: r.seq(() => pageBox(r)), to: linkTo(r) };
}

function page(r: Reader): Page {
  const number = r.varint();
  const side = SIDES[r.varint()];
  if (side === undefined) {
    throw new WireError('a page fell on neither side of the spread');
  }
  return {
    number,
    side,
    width: r.f32(),
    height: r.f32(),
    sections: r.seq(() => r.varint()),
    items: r.seq(() => item(r)),
    links: r.seq(() => link(r)),
  };
}

/**
 * The link under a point on a page, in points from the page's top-left
 * corner, or `null` where there is none. A host that hit tests on its
 * own converts the pointer to page points and asks this, rather than
 * reading the element under the pointer.
 */
export function linkAt(page: Page, x: number, y: number): Link | null {
  return (
    page.links.find((link) =>
      link.areas.some(
        (area) => x >= area.x && x <= area.x + area.width && y >= area.y && y <= area.y + area.height,
      ),
    ) ?? null
  );
}

function font(r: Reader): FontRefEntry {
  return {
    family: r.string(),
    name: r.string(),
    style: r.string(),
    attributes: { italic: r.bool(), weight: r.varint() },
    variations: r.seq(() => ({ tag: r.string(), value: r.f32() })),
  };
}

function asset(r: Reader): Asset {
  return {
    url: r.string(),
    intrinsic: { width: r.varint(), height: r.varint(), dpiX: r.f32(), dpiY: r.f32() },
  };
}

function warning(r: Reader): Warning {
  return { message: r.string(), origin: r.option(() => r.string()) };
}

/**
 * The version a buffer leads with, without reading the rest of it.
 */
export function wireVersionOf(bytes: Uint8Array): number {
  return new Reader(bytes).varint();
}

/**
 * Reads a display structure, refusing a version this reader does not know
 * rather than painting whatever the bytes happen to decode to.
 */
export function decodeDisplayList(bytes: Uint8Array): LayoutOutput {
  const r = new Reader(bytes);
  const version = r.varint();
  if (version !== WIRE_VERSION) {
    throw new WireError(`wire version ${version}, expected ${WIRE_VERSION}`);
  }
  const first = r.varint();
  const bookPages = r.varint();
  const output: LayoutOutput = {
    first,
    bookPages,
    fonts: r.seq(() => font(r)),
    assets: r.seq(() => asset(r)),
    warnings: r.seq(() => warning(r)),
    pages: r.seq(() => page(r)),
  };
  if (!r.done()) {
    throw new WireError('the buffer is more than one display structure');
  }
  return output;
}
