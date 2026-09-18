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

/**
 * The classes and id a sheet reaches a source's sections by, as
 * `section.chapter` and `section#chapter-twelve`.
 */
export interface Attributes {
  /** The classes, without the `.`. */
  classes?: string[];
  /** The id, without the `#`. */
  id?: string;
}

/** One markdown source: what it is called, and what is in it. */
export interface Source {
  /** What the book calls this file, and what an edit replaces. */
  name: string;
  /** Its markdown. */
  text: string;
  /**
   * The classes and id its sections take. They sit beside the text,
   * so no byte of it moves, and the source keeps them when it is
   * edited.
   */
  attributes?: Attributes;
}

/** One stylesheet: what warnings call it, and its CSS. */
export interface Sheet {
  /** The name a warning in this sheet is reported under, as `preset.css:12:3`. */
  name: string;
  /** Its CSS. */
  css: string;
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
  /**
   * Font bytes, kept for the session's life. Without a url, the face
   * registers under the family name in the file. With one, it
   * registers under the family, weight and style of the `@font-face`
   * rule whose `src` names that url, whichever of the two arrives
   * first.
   */
  | { op: 'font'; bytes: Uint8Array; url?: string }
  /**
   * One image, by the url the manuscript names it by, kept for the
   * session's life. The engine opens nothing, so the host fetches
   * the file and the bytes cross once rather than once per render.
   */
  | { op: 'image'; url: string; bytes: Uint8Array }
  /** Which markdown the sources are written in. */
  | { op: 'dialect'; dialect: 'fleuron' | 'commonmark' | 'gfm' | 'obsidian' }
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
  /** The classes and id one source's sections take, with its text left as it is. */
  | { op: 'attributes'; name: string; attributes: Attributes }
  /** A content tree, as JSON, for a host with a structured source of its own. */
  | { op: 'content'; json: string }
  /**
   * The author styling, as named sheets in cascade order. Later
   * sheets win, and a warning names the sheet its declaration was
   * written in.
   */
  | { op: 'style'; sheets: Sheet[] };

/** Where one node of the book was read from. */
export interface NodeSource {
  /** The source the node was read from, by the name the book calls it. */
  source: string;
  /** First byte of that source the node covers. */
  start: number;
  /** One past the last. */
  end: number;
}

/**
 * Where one node's content is set: the folios it runs between, and
 * the pages of the book those folios are.
 *
 * `first` and `last` are printed on the page, and are what a host
 * puts on screen. `at` and `count` are where those pages fall in the
 * book, and are what a host fetches them by. A page counter that
 * restarts makes the two differ, so both are answered.
 */
export interface Folios {
  /** The folio the node's content begins on. */
  first: number;
  /** The folio it ends on, the same number for a node that fits on one page. */
  last: number;
  /** Which page of the book that first folio is, counting from 0, as `Request.first` counts. */
  at: number;
  /** How many pages the content runs across: `{ first: at, count }` is a range over every one. */
  count: number;
}

/** One box on one page, in points from the page's top-left corner. */
export interface PageBox {
  /** Which page of the book, counting from 0, as `Request.first` counts. */
  page: number;
  /** Left edge. */
  x: number;
  /** Top edge. */
  y: number;
  /** Width in points. */
  width: number;
  /** Height in points. */
  height: number;
}

/** One element that an inspected element sits inside. */
export interface Ancestor {
  /** Its node, or `null` for the book and for a table's `thead` and `tbody`. */
  node: number | null;
  /** The element name selectors match. */
  element: string;
  /** The id a selector reaches it by. */
  id: string | null;
  /** The classes a selector reaches it by. */
  classes: string[];
}

/** One declaration of a matched rule. */
export interface InspectedDeclaration {
  /** The property, in lowercase. */
  property: string;
  /** The value as it was written. */
  value: string;
  /** Whether it was written `!important`. */
  important: boolean;
  /** Whether it won the cascade. A shorthand won where any longhand it sets did. */
  applied: boolean;
}

/** One rule that matched. */
export interface MatchedRule {
  /** The name the sheet was handed in under: `user-agent.css` for the built-in sheet. */
  sheet: string;
  /** The line the rule begins on, counting from 1. */
  line: number;
  /** The column it begins at, counting from 1. */
  column: number;
  /** The selector as CSS writes it. For a margin box, the `@page` prelude. */
  selector: string;
  /**
   * Ids, then classes, then element names. For a margin box: a page
   * name, then `:first` or `:blank`, then `:left` or `:right`.
   */
  specificity: [number, number, number];
  /** Its declarations, in the order they were written. */
  declarations: InspectedDeclaration[];
}

/** What styled one element or one margin box, and where it is on the pages. */
export interface Inspection {
  /** The node the answer is about: the element or pseudo-element asked about, or the element that holds the text asked about. `null` for a margin box. */
  node: number | null;
  /** The element name selectors match, or the margin box's at-rule, as `@top-left`. For a pseudo-element, the element it belongs to. */
  element: string;
  /** That element, by its id: the same as `node` for an element, and the element a pseudo-element belongs to. `null` for a margin box. */
  elementNode: number | null;
  /** For a pseudo-element, its name as CSS writes it, as `::first-letter`. */
  pseudoElement?: string;
  /** The id a selector reaches it by. */
  id: string | null;
  /** The classes a selector reaches it by. */
  classes: string[];
  /** The elements it sits inside, the book first. */
  ancestors: Ancestor[];
  /** For a margin box, the page selector its page answers to, as `@page :left`. */
  page?: string;
  /** The rules that matched, in cascade order: where two set the same property, the later one wins. */
  rules: MatchedRule[];
  /** The computed value of every property in the subset, written as CSS, with lengths in points. */
  computed: Record<string, string>;
  /** The border box on each page it reaches. A block split across two pages has two. */
  boxes: PageBox[];
}

/** The page margin boxes, as CSS names them. */
export type MarginBoxName =
  | 'top-left-corner'
  | 'top-left'
  | 'top-center'
  | 'top-right'
  | 'top-right-corner'
  | 'right-top'
  | 'right-middle'
  | 'right-bottom'
  | 'bottom-right-corner'
  | 'bottom-right'
  | 'bottom-center'
  | 'bottom-left'
  | 'bottom-left-corner'
  | 'left-bottom'
  | 'left-middle'
  | 'left-top';

/**
 * What a request wants back, if anything: a display structure, a PDF,
 * the file a face was registered from, the node one byte of a source
 * was read into, the source one node was read from, the folios some
 * nodes are set on, what styled one element or margin box, or the
 * element at a point.
 *
 * The first two are renders and the rest are questions, which is
 * what decides whether a later request may overtake it.
 */
export type Want = 'preview' | 'pdf' | 'font' | 'node' | 'source' | 'folios' | 'inspect' | 'hit';

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
  /** Which source, with `byte`, `want: 'node'` is asking about. */
  source?: string;
  /** Which byte of it. */
  byte?: number;
  /** Which node `want: 'source'` or `want: 'inspect'` is asking about. */
  node?: number;
  /** Which nodes `want: 'folios'` is asking about. */
  nodes?: number[];
  /** Which page, counting from 0, `want: 'hit'` or a margin box's `want: 'inspect'` is asking about. */
  page?: number;
  /** Which margin box of that page `want: 'inspect'` is asking about, in place of `node`. */
  box?: MarginBoxName;
  /** With `y`, the point on the page `want: 'hit'` is asking about, in points from the top-left corner. */
  x?: number;
  /** See {@link Request.x}. */
  y?: number;
  /**
   * With `count`, which pages of `want: 'preview'`'s answer to send:
   * `first` pages counting from 0, `count` of them. Leaving either
   * out asks for the whole book.
   *
   * A ranged preview is a question rather than a render: it answers
   * "what is on this page" of whatever the book already is, not "what
   * did this edit produce", so it neither overtakes nor is overtaken
   * by another render or range in the same batch.
   */
  first?: number;
  /** See {@link Request.first}. */
  count?: number;
}

/**
 * The bytes a request produced: a display structure, a PDF, a font
 * file, or the JSON a question was answered with.
 */
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

/**
 * The style op, written either way: one sheet as CSS text, which the
 * engine calls `author.css`, or the layers a host built its styling
 * out of, in cascade order.
 */
export function styleOp(css: string | Sheet[]): Op {
  return { op: 'style', sheets: typeof css === 'string' ? [{ name: 'author.css', css }] : css };
}
