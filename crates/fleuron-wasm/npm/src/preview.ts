/**
 * A preview, mounted: markdown in, a page on screen.
 *
 * The worker, the module, the postcard buffer and the display list
 * are what this is built out of, not what it asks a caller to hold.
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
import type { Metadata, Op, Response, Source } from './protocol.js';
import { faceFamily, paintPage } from './svg.js';
import type { LayoutOutput, Warning } from './wire.js';

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
   * glyphs land where the display list put them either way, and
   * parsing a book face is main-thread work a page paying for it
   * twice can see.
   */
  faces?: 'module' | 'host';
  /** What the page is printed on; `null` leaves it transparent. */
  paper?: string | null;
  /** What it is printed in. */
  ink?: string;
  /** Where an image's pixels come from. */
  asset?: (index: number) => string | null | undefined;
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
   * Names the book. Only the PDF export reads this, so it costs no
   * layout.
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

  /** Sets the author stylesheet, cascading over the built-in one. */
  async setStyle(css: string): Promise<void> {
    await this.render([{ op: 'style', css }]);
  }

  /** Registers a face for the session's life. */
  async addFont(bytes: Uint8Array): Promise<void> {
    await this.render([{ op: 'font', bytes }]);
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
    this.element.replaceChildren();
    this.worker.terminate();
  }

  private paint(): void {
    this.frame.innerHTML = this.svg();
  }

  private painting() {
    return {
      fonts: this.output?.fonts ?? [],
      zoom: this.scale,
      ...(this.options.paper === undefined ? {} : { paper: this.options.paper }),
      ...(this.options.ink === undefined ? {} : { ink: this.options.ink }),
      ...(this.options.asset === undefined ? {} : { asset: this.options.asset }),
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
