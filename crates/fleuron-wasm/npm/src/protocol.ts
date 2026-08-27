/**
 * What crosses between a host and the worker the engine runs in.
 *
 * A request is an edit plus, sometimes, a render: inputs travel when
 * they change rather than once per frame, and the module keeps every
 * stage between them. Each request carries a generation, which the
 * worker echoes back untouched. The host raises it whenever the
 * input goes stale, and a reply that comes back behind the current
 * one is dropped rather than painted.
 */

/** One input reaching the engine. */
export type Op =
  /** Font bytes, registered once and kept for the session's life. */
  | { op: 'font'; bytes: Uint8Array }
  /** Which markdown the sources are written in. */
  | { op: 'dialect'; dialect: 'commonmark' | 'gfm' | 'obsidian' }
  /** Where a source's sections begin: a heading level, or 0 for a file per section. */
  | { op: 'split'; level: number }
  /** One markdown source as the whole book. */
  | { op: 'markdown'; name: string; text: string }
  /** One source replaced: the keystroke path, where the rest of the book stands. */
  | { op: 'edit'; name: string; text: string }
  /** A content tree, as JSON, for a host with a structured source of its own. */
  | { op: 'content'; json: string }
  /** The author stylesheet, as CSS text. */
  | { op: 'style'; css: string };

/** What a request wants back, if anything. */
export type Want = 'preview' | 'pdf';

/** An edit, a render, or both. */
export interface Request {
  /** Pairs the reply with the call. */
  id: number;
  /** Raised by the host whenever the input goes stale. */
  generation: number;
  /** The inputs that changed, applied in the order they arrive. */
  ops: Op[];
  /** What to send back once they have been applied. */
  want?: Want;
}

/** The bytes a render produced: a display list, or a PDF. */
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

/** Whether a reply carries bytes. */
export function isRendered(response: Response): response is Rendered {
  return 'bytes' in response;
}

/** Whether a reply is the engine reporting trouble. */
export function isFailed(response: Response): response is Failed {
  return 'error' in response;
}
