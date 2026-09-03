/**
 * The wire: postcard bytes in, a display structure out.
 *
 * The engine encodes with postcard, which sends no field names and
 * packs small numbers into one byte. Nothing in the buffer says what
 * it is, so this reader walks the same fields in the same order the
 * engine wrote them, and the version in front of the bytes is what
 * catches the day those two stop agreeing.
 */

/** The encoding this reader understands. */
export const WIRE_VERSION = 4;

/** Which side of the spread a page falls on. */
export type Side = 'recto' | 'verso';

/** One glyph: an id in its font, an absolute x, and the text it stands for. */
export interface Glyph {
  /** Glyph id in the run's font. */
  id: number;
  /** Absolute x of the glyph's origin, in points. */
  x: number;
  /** Byte range in the run's text this glyph came from. */
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
   * The text the glyphs were shaped from. Only the shaper knew which
   * glyph came from which character, so the correspondence travels
   * with them: selection, copy-and-paste and accessible text read it
   * through each glyph's range.
   */
  text: string;
  /** The glyphs, in visual order. */
  glyphs: Glyph[];
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
}

/** A single paint operation. */
export type DrawItem = TextItem | RectItem | ImageItem;

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
   * The content-tree node ids of the sections this page holds content
   * from, in the order their content appears on it. A chapter that
   * ends mid-page is followed there by the next one opening, so the
   * page names both. A blank leaf names none.
   */
  sections: number[];
  /** What to paint, in paint order. */
  items: DrawItem[];
}

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

/** A font's identity, as the display structure indexes it. */
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
  /** Where it was written, when the engine knows. */
  origin: string | null;
}

/** Everything one run produced: pages, the fonts they index, diagnostics. */
export interface LayoutOutput {
  /** The typeset pages, in reading order. */
  pages: Page[];
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
        glyphs: r.seq(() => glyph(r)),
      };
    case 1:
      return { kind: 'rect', x: r.f32(), y: r.f32(), w: r.f32(), h: r.f32() };
    case 2:
      return {
        kind: 'image',
        x: r.f32(),
        y: r.f32(),
        w: r.f32(),
        h: r.f32(),
        asset: r.varint(),
      };
    default:
      throw new WireError(`draw item ${variant} is not one this reader knows`);
  }
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
  };
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
  const output: LayoutOutput = {
    pages: r.seq(() => page(r)),
    fonts: r.seq(() => font(r)),
    assets: r.seq(() => asset(r)),
    warnings: r.seq(() => warning(r)),
  };
  if (!r.done()) {
    throw new WireError('the buffer holds more than one display structure');
  }
  return output;
}
