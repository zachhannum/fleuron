/**
 * What crosses between a host and the worker the engine runs in.
 *
 * A request is an edit plus, sometimes, a render: inputs travel when
 * they change rather than once per frame, and the module keeps every
 * stage between them. Each request names a generation, which the
 * worker echoes back untouched. The host raises it whenever the
 * input goes stale, and a reply that comes back behind the current
 * one is dropped rather than painted.
 */

/** One markdown source: what it is called, and what is in it. */
export interface Source {
  /** What the book calls this file, and what an edit replaces. */
  name: string;
  /** Its markdown. */
  text: string;
}

/** What names a book: its title, its author, and whatever else. */
export interface Metadata {
  /** Title, for the half-title and running heads. */
  title?: string;
  /** Author, for the title page. */
  author?: string;
  /** Anything else a frontend read, such as `language`. */
  extra?: Record<string, string>;
}

/** One input reaching the engine. */
export type Op =
  /** Font bytes, registered once and kept for the session's life. */
  | { op: 'font'; bytes: Uint8Array }
  /**
   * One image, by the url the manuscript names it by, kept for the
   * session's life. The engine opens nothing, so the host fetches
   * the file and the bytes cross once rather than once per render.
   */
  | { op: 'image'; url: string; bytes: Uint8Array }
  /** Which markdown the sources are written in. */
  | { op: 'dialect'; dialect: 'commonmark' | 'gfm' | 'obsidian' }
  /** Where a source's sections begin: a heading level, or 0 for a file per section. */
  | { op: 'split'; level: number }
  /** One markdown source as the whole book. */
  | { op: 'markdown'; name: string; text: string }
  /** Every markdown source of a book, in reading order. */
  | { op: 'book'; sources: Source[] }
  /** One source dropped, and the rest of the book left standing. */
  | { op: 'remove'; name: string }
  /** What names the book, for a book whose sources cannot say. */
  | { op: 'metadata'; metadata: Metadata }
  /** One source replaced: the keystroke path, where the rest of the book stands. */
  | { op: 'edit'; name: string; text: string }
  /** A content tree, as JSON, for a host with a structured source of its own. */
  | { op: 'content'; json: string }
  /** The author stylesheet, as CSS text. */
  | { op: 'style'; css: string };

/**
 * What a request wants back, if anything: a display structure, a PDF, or
 * the file a face was registered from.
 *
 * The first two are renders and the last is a question, which is
 * what decides whether a later request may overtake it.
 */
export type Want = 'preview' | 'pdf' | 'font';

/** An edit, a render, a question, or an edit and one of those. */
export interface Request {
  /** Pairs the reply with the call. */
  id: number;
  /** Raised by the host whenever the input goes stale. */
  generation: number;
  /** The inputs that changed, applied in the order they arrive. */
  ops: Op[];
  /** What to send back once they have been applied. */
  want?: Want;
  /** Which face `want: 'font'` is asking for. */
  font?: number;
}

/** The bytes a request produced: a display structure, a PDF, or a font file. */
export interface Rendered {
  id: number;
  generation: number;
  kind: Want;
  bytes: Uint8Array;
  /**
   * How many times each stage has run since the session opened, as
   * `[style, lines, flow, paint]`. What the edit cost, in stage runs
   * rather than milliseconds: a cache that served shows here, where
   * a clock would only show a fast machine.
   */
  stages: [number, number, number, number];
}

/** Inputs applied, with nothing asked for back. */
export interface Applied {
  id: number;
  generation: number;
  applied: true;
}

/**
 * A render another request overtook before it ran. Its inputs were
 * applied; only the painting was skipped.
 */
export interface Superseded {
  id: number;
  generation: number;
  superseded: true;
}

/** A request the engine refused, and what it said. */
export interface Failed {
  id: number;
  generation: number;
  error: string;
}

/** What comes back for a request. */
export type Response = Rendered | Applied | Superseded | Failed;

/** Whether a reply came back with bytes. */
export function isRendered(response: Response): response is Rendered {
  return 'bytes' in response;
}

/** Whether a reply is the engine reporting trouble. */
export function isFailed(response: Response): response is Failed {
  return 'error' in response;
}
