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
import { styleOp, type Metadata, type Op, type Response, type Sheet, type Source } from './protocol.js';
import { faceFamily, paintPage } from './svg.js';
import type { Asset, LayoutOutput, Warning } from './wire.js';

/** How a preview is set up. */
export interface PreviewOptions {
  /**
   * The worker to run layout in. The package's own is used when
   * there is none, which is what a host that has nothing to add to
   * it wants.
   */
  worker?: Worker;
  /** Which markdown the sources are written in. */
  dialect?: 'commonmark' | 'gfm' | 'obsidian';
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
  /** Called after every render that reached the screen. */
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
  /** A blob url per image the host handed over, by its own url. */
  private readonly pixels = new Map<string, string>();
  private output: LayoutOutput | null = null;
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

  /** Lays the book out again and repaints. */
  async render(ops: Op[] = []): Promise<void> {
    const output = await this.client.preview(ops);
    if (output === null) {
      return;
    }
    this.output = output;
    await this.load(output);
    this.paint();
    this.options.onRender?.(output);
  }

  /** How many pages the book set to. */
  get pages(): number {
    return this.output?.pages.length ?? 0;
  }

  /** The page on screen, counting from 1. */
  get page(): number {
    return this.showing;
  }

  set page(number: number) {
    this.showing = Math.min(Math.max(Math.round(number), 1), Math.max(this.pages, 1));
    this.paint();
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
    return this.output?.warnings ?? [];
  }

  /**
   * The markup on screen, or a page that is not on screen. Empty
   * before the first render.
   */
  svg(number = this.showing): string {
    const page = this.output?.pages[number - 1];
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
    for (const face of this.faces.values()) {
      this.element.ownerDocument.fonts.delete(face);
    }
    this.faces.clear();
    for (const url of this.pixels.values()) {
      URL.revokeObjectURL(url);
    }
    this.pixels.clear();
    this.element.replaceChildren();
    this.worker.terminate();
  }

  /**
   * Keeps a blob url over an image's bytes and hands the same bytes
   * to the engine.
   *
   * The blob is made before the op is sent, because the bytes move
   * across the wall rather than being copied and the buffer is empty
   * on this side afterwards.
   */
  private keepImage(url: string, bytes: Uint8Array): Op {
    const previous = this.pixels.get(url);
    if (previous !== undefined) {
      URL.revokeObjectURL(previous);
    }
    this.pixels.set(url, URL.createObjectURL(new Blob([bytes.slice()], { type: mediaType(bytes) })));
    return { op: 'image', url, bytes };
  }

  private paint(): void {
    this.frame.innerHTML = this.svg();
  }

  private painting() {
    return {
      fonts: this.output?.fonts ?? [],
      assets: this.output?.assets ?? [],
      zoom: this.scale,
      ...(this.options.paper === undefined ? {} : { paper: this.options.paper }),
      ...(this.options.ink === undefined ? {} : { ink: this.options.ink }),
      asset: this.options.asset ?? ((asset: Asset) => this.pixels.get(asset.url)),
    };
  }

  /**
   * Loads the faces the run drew with, from the same files the
   * engine shaped with.
   *
   * The bundled face is why this asks the module rather than the
   * network: it is inside the module, and there is no URL to fetch
   * it from. A face whose bytes do not come back is left out, and
   * the painter's fallback stack is what the reader sees instead of
   * a blank page. `faces: 'host'` is that stack on purpose.
   */
  private async load(output: LayoutOutput): Promise<void> {
    if (this.options.faces === 'host') {
      return;
    }
    const used = new Set<number>();
    for (const page of output.pages) {
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
      const attributes = this.output?.fonts[id]?.attributes;
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
