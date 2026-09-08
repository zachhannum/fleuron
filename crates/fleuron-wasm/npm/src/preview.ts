/**
 * A preview, mounted: markdown in, a page on screen.
 *
 * The worker, the module, the postcard buffer and the display structure
 * are what this is built out of, not what it asks a caller to supply.
 * A host that wants them keeps having them, since the client, the
 * protocol, the reader and the painter are all still exported. A host
 * that only wants to see the book says where to put it and hands over
 * the manuscript.
 *
 * One page at a time, because that is what a page-through needs and
 * what a scrolling preview would have to virtualise anyway.
 *
 * Every method that changes an input renders. Nothing here is on a
 * timer: renders that pile up while the engine is busy collapse into
 * one, and an engine with nothing to do repaints straight away. A
 * host that wants a fixed delay puts one in front of these calls.
 */

import { Client, type Transport } from './client.js';
import {
  styleOp,
  type Metadata,
  type Op,
  type Response,
  type Sheet,
  type Source,
} from './protocol.js';
import { faceFamily, paintPage } from './svg.js';
import type { Asset, FontRefEntry, LayoutOutput, Page, Warning } from './wire.js';

/**
 * How many pages either side of the one on screen a preview keeps
 * decoded. A page turn within this radius paints from what is
 * already held; one past it asks the worker.
 */
const HOLD_RADIUS = 1;

/** How a preview is set up. */
export interface PreviewOptions {
  /**
   * The worker to run layout in. The package's own is used when
   * there is none, which is what a host that has nothing to add to
   * it wants.
   */
  worker?: Worker;
  /** Which markdown the sources are written in. */
  dialect?: 'fleuron' | 'commonmark' | 'gfm' | 'obsidian';
  /** The heading level a section begins at, or 0 for one per file. */
  split?: number;
  /** Points to CSS pixels. */
  zoom?: number;
  /** The page to show, counting from 1. */
  page?: number;
  /**
   * Where the faces on screen come from.
   *
   * `module` asks the session for the file it shaped with and
   * registers that, which is the only way to be certain the page on
   * screen is set in the face the export will use.
   *
   * `host` registers nothing and leaves the painter's family stack
   * to resolve against whatever the document already has. A host
   * that already serves the same file, or a subset of it with the
   * same metrics, uses this rather than fetch a second copy: the
   * glyphs land where the display structure put them either way, and
   * parsing a book face is main-thread work a page paying for it
   * twice can see.
   */
  faces?: 'module' | 'host';
  /** What the page is printed on; `null` leaves it transparent. */
  paper?: string | null;
  /** What it is printed in. */
  ink?: string;
  /**
   * The images the manuscript refers to, by the url it names them
   * by. Nothing here fetches a url, so a host that wants an image on
   * the page fetches the file itself and hands over the bytes.
   *
   * The same bytes both size the box and fill it: the module reads
   * the header, and the painter draws from a blob url over the file
   * that header came from.
   */
  images?: Record<string, Uint8Array>;
  /**
   * Where an image's pixels come from, for a host that would rather
   * name its own urls than hand over bytes. This outranks whatever
   * {@link PreviewOptions.images} supplied.
   */
  asset?: (asset: Asset, index: number) => string | null | undefined;
  /**
   * Called after every render that reached the screen, with the
   * reply the page on screen came from. `output.pages` is that page
   * alone, not the whole book: `output.bookPages` is the book's own
   * length, and `output.fonts`/`output.assets`/`output.warnings` are
   * the whole run's regardless.
   */
  onRender?: (output: LayoutOutput) => void;
}

/**
 * A book on screen, and the worker behind it.
 *
 * Every method that changes an input renders and repaints. A render
 * the caller has already typed past paints nothing, so calling these
 * on a keystroke is the intended use rather than a risk.
 */
export class Preview {
  private readonly element: Element;
  private readonly frame: Element;
  private readonly worker: Worker;
  private readonly client: Client;
  private readonly options: PreviewOptions;
  private readonly faces = new Map<number, FontFace>();
  /**
   * Every image's own bytes, by url, kept independent of the transfer
   * that moved the ones sent across the wall: a copy taken before the
   * op went out, since the buffer handed to the engine is empty on
   * this side afterwards.
   */
  private readonly imageBytes = new Map<string, Uint8Array>();
  /**
   * A blob url per image currently drawn by a held page, by its own
   * url. Built and revoked by {@link syncAssetCache}: a book with
   * more images than are ever on screen at once does not keep a blob
   * open, and its decode, for every one of them.
   */
  private readonly pixels = new Map<string, string>();
  /**
   * The pages this preview currently keeps decoded, by folio. Bounded
   * to a window around the page on screen rather than the whole
   * book: what makes a page turn instant is prefetching a neighbour
   * before it is asked for, not holding every page there is.
   */
  private readonly held = new Map<number, Page>();
  /**
   * The generation `held` was built under. A reply for a different
   * generation means an edit landed, and drops whatever was held
   * before inserting: a page from the book before the edit is not
   * one to paint over it.
   */
  private heldGeneration = -1;
  /**
   * Pages currently being asked for from the worker, whether the
   * page turned to or a background prefetch of a neighbour, to the
   * generation they were asked for under. Asking again for one
   * already here — including a page left and come back to before its
   * first request landed — joins it rather than opening a second
   * request for the same page.
   *
   * Keyed on generation, not just the folio, so a fetch left over
   * from the generation before an edit cannot clear the marker a
   * fresh fetch for the same folio placed under the new one when the
   * old one finally lands and finds itself answering a book that no
   * longer stands.
   */
  private readonly pending = new Map<number, number>();
  /** The whole run's tables and diagnostics, which ride every reply
   * regardless of which pages it carried. */
  private fonts: FontRefEntry[] = [];
  private assets: Asset[] = [];
  private bookWarnings: Warning[] = [];
  private bookPages = 0;
  private showing: number;
  private scale: number;

  private constructor(element: Element, worker: Worker, options: PreviewOptions) {
    this.element = element;
    this.worker = worker;
    this.options = options;
    this.showing = options.page ?? 1;
    this.scale = options.zoom ?? 1;
    this.frame = element.ownerDocument.createElement('div');
    this.frame.setAttribute('data-fleuron', 'preview');
    element.replaceChildren(this.frame);
    const transport: Transport = {
      post: (request, transfer) => worker.postMessage(request, transfer),
    };
    this.client = new Client(transport);
    worker.addEventListener('message', (event) =>
      this.client.receive((event as MessageEvent<Response>).data),
    );
    this.element.addEventListener('copy', this.onCopy);
  }

  /**
   * Opens a worker, loads the module into it, and takes the element
   * over. Nothing is painted until a manuscript arrives.
   */
  static async mount(element: Element, options: PreviewOptions = {}): Promise<Preview> {
    const worker =
      options.worker ??
      new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
    const preview = new Preview(element, worker, options);
    const setup: Op[] = [];
    if (options.dialect !== undefined) {
      setup.push({ op: 'dialect', dialect: options.dialect });
    }
    if (options.split !== undefined) {
      setup.push({ op: 'split', level: options.split });
    }
    for (const [url, bytes] of Object.entries(options.images ?? {})) {
      setup.push(preview.keepImage(url, bytes));
    }
    if (setup.length > 0) {
      await preview.client.apply(setup);
    }
    return preview;
  }

  /** Sets the manuscript: one markdown source as the whole book. */
  async setMarkdown(text: string, name = 'manuscript.md'): Promise<void> {
    await this.render([{ op: 'markdown', name, text }]);
  }

  /**
   * Sets the manuscript from several sources, in reading order.
   *
   * A book of one file takes its title and author from that file's
   * frontmatter. A book of several has no frontmatter of its own, so
   * it is left unnamed until {@link setMetadata} names it, rather
   * than named after whichever chapter came first.
   */
  async setBook(sources: Source[]): Promise<void> {
    await this.render([{ op: 'book', sources }]);
  }

  /** Drops one source and leaves the rest of the book standing. */
  async remove(name: string): Promise<void> {
    await this.render([{ op: 'remove', name }]);
  }

  /**
   * Names the book. The PDF export reads the name, and layout reads
   * `extra.language`, which chooses the hyphenation patterns, so a
   * rename alone costs no layout.
   */
  async setMetadata(metadata: Metadata): Promise<void> {
    await this.render([{ op: 'metadata', metadata }]);
  }

  /**
   * Replaces one source and leaves the rest of the book standing.
   * This is the keystroke path: the sections that came from this
   * file are read again and every other file keeps the lines it
   * already has.
   *
   * A name the book has not seen is appended, which is one way to
   * add a file to a book already open.
   */
  async edit(name: string, text: string): Promise<void> {
    await this.render([{ op: 'edit', name, text }]);
  }

  /**
   * Sets the author styling, cascading over the built-in sheet:
   * one sheet as CSS text, or the layers a host built its styling
   * out of, in cascade order. Later sheets win, and a warning names
   * the sheet its declaration was written in.
   */
  async setStyle(css: string | Sheet[]): Promise<void> {
    await this.render([styleOp(css)]);
  }

  /** Registers a face for the session's life. */
  async addFont(bytes: Uint8Array): Promise<void> {
    await this.render([{ op: 'font', bytes }]);
  }

  /**
   * Registers one image, by the url the manuscript names it by, and
   * lays the book out again around the room it takes.
   *
   * A url the manuscript names and nobody supplies is a diagnostic
   * and a page with nothing where the image was.
   */
  async addImage(url: string, bytes: Uint8Array): Promise<void> {
    await this.render([this.keepImage(url, bytes)]);
  }

  /**
   * Lays the book out again and repaints, asking only for the page on
   * screen rather than the whole book. An edit drops whatever pages
   * were held, since the book they came from no longer stands.
   */
  async render(ops: Op[] = []): Promise<void> {
    const generation = this.client.generationFor(ops);
    const target = Math.max(this.showing, 1);
    const reply = await this.client.preview(ops, { first: target - 1, count: 1 });
    if (reply === null) {
      return;
    }
    this.absorb(reply, generation);
    this.showing = Math.min(Math.max(this.showing, 1), Math.max(this.bookPages, 1));
    if (!this.held.has(this.showing)) {
      // The edit changed how many pages the book has, and the page
      // that was asked for clamped to one this reply did not carry.
      const fix = await this.client.preview([], { first: this.showing - 1, count: 1 });
      if (fix !== null && generation === this.heldGeneration) {
        this.absorb(fix, generation);
      }
    }
    this.prune();
    this.syncAssetCache();
    await this.load();
    this.paint();
    this.notify();
    this.prefetchNeighbours();
  }

  /** How many pages the book set to. */
  get pages(): number {
    return this.bookPages;
  }

  /** The page on screen, counting from 1. */
  get page(): number {
    return this.showing;
  }

  set page(number: number) {
    const clamped = Math.min(Math.max(Math.round(number), 1), Math.max(this.pages, 1));
    if (clamped === this.showing && this.held.has(clamped)) {
      // Already on screen and painted: a render already notified for
      // it, and a host that re-assigns the same page on every one of
      // its own re-renders (a React effect keyed on the page it
      // reads back, say) must not see that turn into a render of its
      // own. `held` is checked rather than just the number, so a
      // jump that failed outright (the worker answered with an
      // error, say) — holding nothing — is still retried by asking
      // again, rather than stuck with no way back short of
      // navigating off the page and back.
      return;
    }
    this.showing = clamped;
    if (this.held.has(clamped)) {
      // Already decoded, from an earlier prefetch or an edit that
      // requested it directly: paints without asking the worker.
      this.prune();
      this.syncAssetCache();
      this.paint();
      this.notify();
    } else {
      // Not held: the frame stays as it is until the page asked for
      // arrives, rather than blank in the meantime. `fetchPage` joins
      // a request already in flight for it — a page left and come
      // back to before its own request landed, or one already asked
      // for as a neighbour's prefetch — and paints it once it lands,
      // since by then it may be the page on screen whichever request
      // brought it in.
      this.fetchPage(clamped);
    }
    this.prefetchNeighbours();
  }

  /** Points to CSS pixels. */
  get zoom(): number {
    return this.scale;
  }

  set zoom(scale: number) {
    this.scale = scale;
    this.paint();
  }

  /** The next page, if the book has one. */
  next(): void {
    this.page = this.showing + 1;
  }

  /** The previous page, if there is one. */
  previous(): void {
    this.page = this.showing - 1;
  }

  /** Everything the run had to complain about. */
  get warnings(): Warning[] {
    return this.bookWarnings;
  }

  /**
   * The markup on screen, or a page that is not held. Empty before
   * the first render, and for a page outside the held window.
   */
  svg(number = this.showing): string {
    const page = this.held.get(number);
    return page === undefined ? '' : paintPage(page, this.painting());
  }

  /**
   * The book as PDF bytes, from the stages the preview settled, so
   * the export cannot contradict what is on screen.
   */
  async exportPdf(): Promise<Uint8Array | null> {
    return this.client.exportPdf();
  }

  /** Closes the worker and gives the element back. */
  destroy(): void {
    this.element.removeEventListener('copy', this.onCopy);
    for (const face of this.faces.values()) {
      this.element.ownerDocument.fonts.delete(face);
    }
    this.faces.clear();
    for (const url of this.pixels.values()) {
      URL.revokeObjectURL(url);
    }
    this.pixels.clear();
    this.imageBytes.clear();
    this.held.clear();
    this.element.replaceChildren();
    this.worker.terminate();
  }

  /**
   * Keeps a copy of an image's bytes and hands the original to the
   * engine.
   *
   * The copy is taken before the op is sent, because the bytes move
   * across the wall rather than being copied and the buffer is empty
   * on this side afterwards. No blob is made here: {@link
   * syncAssetCache} makes one only once a held page actually draws
   * this url, which for most images in a book-sized manuscript is
   * never at the same time as every other one.
   */
  private keepImage(url: string, bytes: Uint8Array): Op {
    const previous = this.pixels.get(url);
    if (previous !== undefined) {
      // Stale bytes replaced: the blob over them answers for an
      // image that no longer exists, so it goes now rather than
      // waiting on a page that may never come held again to notice.
      URL.revokeObjectURL(previous);
      this.pixels.delete(url);
    }
    this.imageBytes.set(url, bytes.slice());
    return { op: 'image', url, bytes };
  }

  /**
   * Fetches one page not already pending, absorbing it and painting
   * it if it is — or by the time it lands, has become — the page on
   * screen. Shared by a page turn and a neighbour prefetch, so
   * whichever of them asks first, the other joins it rather than
   * opening a second request for the same page: what the request was
   * *for* is decided when it lands, by whether `showing` still names
   * it, not by which caller happened to start it.
   *
   * Errors are swallowed the way a missing face is elsewhere:
   * nothing here is awaited by a caller that could catch one, and the
   * frame simply stays as it was, retried the next time this page is
   * asked for.
   */
  private fetchPage(target: number): void {
    if (this.pending.has(target)) {
      return;
    }
    const generation = this.heldGeneration;
    this.pending.set(target, generation);
    this.client
      .preview([], { first: target - 1, count: 1 })
      .then(async (reply) => {
        if (reply === null || generation !== this.heldGeneration) {
          return;
        }
        this.absorb(reply, generation);
        // Pruned whether or not this is the page on screen: `showing`
        // may have moved on again while this was in flight, and its
        // own window is what a page fetched for a target that far
        // away landed outside of.
        this.prune();
        this.syncAssetCache();
        if (this.showing !== target) {
          return;
        }
        await this.load();
        this.paint();
        this.notify();
      })
      .catch(() => undefined)
      .finally(() => {
        // Only clears the marker this fetch itself placed: a
        // generation change may have cleared `pending` outright and
        // let a fresh fetch for the same folio claim it under the
        // new generation, which this must not remove out from under
        // it.
        if (this.pending.get(target) === generation) {
          this.pending.delete(target);
        }
      });
  }

  /**
   * Warms the pages either side of the one on screen, so a page turn
   * in either direction paints from what is already held rather than
   * asking the worker.
   */
  private prefetchNeighbours(): void {
    for (const target of [this.showing - 1, this.showing + 1]) {
      if (target >= 1 && target <= this.bookPages && !this.held.has(target)) {
        this.fetchPage(target);
      }
    }
  }

  /**
   * Folds a reply into the held pages, keyed by folio rather than by
   * its position in the reply. A reply for a generation `held` was
   * not built under drops whatever was held before inserting: an
   * edit is not undone by a page fetched against the book before it.
   */
  private absorb(reply: LayoutOutput, generation: number): void {
    if (generation !== this.heldGeneration) {
      this.held.clear();
      // A fetch still in flight for the generation before this one
      // is answering a question about a book that no longer stands;
      // its own generation check will discard the reply, but leaving
      // its target `pending` until then would dedupe away a fresh
      // fetch for the same folio in this generation, silently
      // skipping it rather than prefetching it anew.
      this.pending.clear();
      this.heldGeneration = generation;
    }
    this.fonts = reply.fonts;
    this.assets = reply.assets;
    this.bookWarnings = reply.warnings;
    this.bookPages = reply.bookPages;
    reply.pages.forEach((page, index) => {
      this.held.set(reply.first + index + 1, page);
    });
  }

  /** Drops whatever is held outside the window around the page on screen. */
  private prune(): void {
    const low = this.showing - HOLD_RADIUS;
    const high = this.showing + HOLD_RADIUS;
    for (const folio of this.held.keys()) {
      if (folio < low || folio > high) {
        this.held.delete(folio);
      }
    }
  }

  /**
   * Blobs every image a held page draws that does not have one yet,
   * and revokes every blob no held page draws any more. Run after
   * {@link prune}, over `held` as a whole rather than page by page:
   * an asset two held pages share is not revoked for one of them
   * dropping out while the other still stands.
   *
   * A url the host has not supplied bytes for is simply absent from
   * `imageBytes`, the same gap the painter's asset resolver already
   * falls back from — this adds no failure mode of its own, only an
   * eviction of what was already optional.
   */
  private syncAssetCache(): void {
    const needed = new Set<string>();
    for (const page of this.held.values()) {
      for (const item of page.items) {
        if (item.kind === 'image') {
          const url = this.assets[item.asset]?.url;
          if (url !== undefined) {
            needed.add(url);
          }
        }
      }
    }
    for (const url of needed) {
      if (!this.pixels.has(url)) {
        const bytes = this.imageBytes.get(url);
        if (bytes !== undefined) {
          this.pixels.set(
            url,
            URL.createObjectURL(new Blob([bytes.slice()], { type: mediaType(bytes) })),
          );
        }
      }
    }
    for (const [url, blobUrl] of this.pixels) {
      if (!needed.has(url)) {
        URL.revokeObjectURL(blobUrl);
        this.pixels.delete(url);
      }
    }
  }

  private paint(): void {
    this.frame.innerHTML = this.svg();
  }

  /**
   * Copies the selection layer's own text, reconstructed from the
   * range's own boundaries rather than trusted to the browser's
   * default serialization across `<text>` siblings — SVG text
   * elements are not block boxes, and nothing guarantees a browser
   * puts a line break between two of them the way it would between
   * paragraphs. This is the pdf.js pattern: the layer under the
   * pointer draws nothing, and copy answers from what it holds.
   */
  private readonly onCopy = (event: Event): void => {
    const text = this.selectedText();
    if (text === null) {
      return;
    }
    (event as ClipboardEvent).clipboardData?.setData('text/plain', text);
    event.preventDefault();
  };

  /**
   * The selection's own text, one line's slice per line it touches,
   * joined in reading order. `null` for a selection with nothing in
   * it, or one that lies outside this preview altogether — a host's
   * own text elsewhere on the page is not this preview's to answer
   * for.
   */
  private selectedText(): string | null {
    const selection = this.element.ownerDocument.getSelection();
    if (selection === null || selection.isCollapsed || selection.rangeCount === 0) {
      return null;
    }
    const range = selection.getRangeAt(0);
    if (!this.frame.contains(range.commonAncestorContainer)) {
      return null;
    }
    const lines = [...this.frame.querySelectorAll('text[data-selection-line]')] as SVGTextElement[];
    const parts: string[] = [];
    for (const line of lines) {
      if (!range.intersectsNode(line)) {
        continue;
      }
      const full = line.textContent ?? '';
      const start = line.contains(range.startContainer)
        ? boundaryOffset(line, range.startContainer, range.startOffset)
        : 0;
      const end = line.contains(range.endContainer)
        ? boundaryOffset(line, range.endContainer, range.endOffset)
        : full.length;
      parts.push(full.slice(start, end));
    }
    return parts.length === 0 ? null : parts.join('\n');
  }

  /**
   * Calls {@link PreviewOptions.onRender}, if the page on screen is
   * held: built fresh from `held` and the run's tables rather than
   * threaded through from whichever fetch put it there, so a host
   * hears about every page that reaches the screen the same way,
   * cached or just arrived.
   */
  private notify(): void {
    const page = this.held.get(this.showing);
    if (page === undefined) {
      return;
    }
    this.options.onRender?.({
      pages: [page],
      first: this.showing - 1,
      bookPages: this.bookPages,
      fonts: this.fonts,
      assets: this.assets,
      warnings: this.bookWarnings,
    });
  }

  private painting() {
    return {
      fonts: this.fonts,
      assets: this.assets,
      zoom: this.scale,
      ...(this.options.paper === undefined ? {} : { paper: this.options.paper }),
      ...(this.options.ink === undefined ? {} : { ink: this.options.ink }),
      asset: this.options.asset ?? ((asset: Asset) => this.pixels.get(asset.url)),
    };
  }

  /**
   * Loads the faces the held pages draw with, from the same files
   * the engine shaped with.
   *
   * The bundled face is why this asks the module rather than the
   * network: it is inside the module, and there is no URL to fetch
   * it from. A face whose bytes do not come back is left out, and
   * the painter's fallback stack is what the reader sees instead of
   * a blank page. `faces: 'host'` is that stack on purpose.
   */
  private async load(): Promise<void> {
    if (this.options.faces === 'host') {
      return;
    }
    const used = new Set<number>();
    for (const page of this.held.values()) {
      for (const item of page.items) {
        if (item.kind === 'text') {
          used.add(item.fontId);
        }
      }
    }
    const wanted = [...used].filter((id) => !this.faces.has(id));
    await Promise.all(wanted.map((id) => this.face(id)));
  }

  private async face(id: number): Promise<void> {
    try {
      const bytes = await this.client.fontBytes(id);
      // Registered as what it is, so that asking for it by that
      // slope and weight is an exact match and the browser
      // synthesises nothing over the cut the engine shaped with.
      const attributes = this.fonts[id]?.attributes;
      const face = new FontFace(faceFamily(id), bytes.buffer as ArrayBuffer, {
        style: attributes?.italic === true ? 'italic' : 'normal',
        weight: String(attributes?.weight ?? 400),
      });
      await face.load();
      this.faces.set(id, face);
      this.element.ownerDocument.fonts.add(face);
    } catch {
      // A face that will not load is one the painter falls back
      // from, which is visible on the page and needs no throw here.
    }
  }
}

/**
 * A range boundary's offset into one selection line's text, whichever
 * kind of node it landed on. Inside the line's own text node the
 * offset is already a character index; on the `<text>` element
 * itself — which has exactly that one child — it is a child index, 0
 * before it and 1 after, so it becomes the two ends of the line's own
 * text rather than a stray zero.
 */
function boundaryOffset(line: SVGTextElement, container: Node, offset: number): number {
  return container === line.firstChild ? offset : offset === 0 ? 0 : (line.textContent ?? '').length;
}

/**
 * What an image is, read off its own first bytes.
 *
 * A blob with no type is sniffed by the browser in some places and
 * refused in others, and the file already says what it is in the
 * same signature the engine probed it by.
 */
function mediaType(bytes: Uint8Array): string {
  const starts = (...signature: number[]): boolean =>
    signature.every((byte, at) => bytes[at] === byte);
  if (starts(0x89, 0x50, 0x4e, 0x47)) {
    return 'image/png';
  }
  if (starts(0xff, 0xd8)) {
    return 'image/jpeg';
  }
  if (starts(0x47, 0x49, 0x46, 0x38)) {
    return 'image/gif';
  }
  if (starts(0x52, 0x49, 0x46, 0x46)) {
    return 'image/webp';
  }
  return 'application/octet-stream';
}
