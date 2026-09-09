/**
 * The host's side of the wall: send an edit, get a display structure, and
 * never paint one the reader has already typed past.
 */

import {
  isFailed,
  isRendered,
  type Folios,
  type NodeSource,
  type Op,
  type Request,
  type Response,
  type Want,
} from './protocol.js';
import { decodeDisplayList, type LayoutOutput } from './wire.js';

/** How a request reaches the worker. */
export interface Transport {
  /**
   * Sends one request. `transfer` lists the buffers that should move
   * rather than be copied: font and image bytes, which the host has
   * no reason to keep a second copy of.
   */
  post(request: Request, transfer: ArrayBuffer[]): void;
}

/** A render that was overtaken: nothing came back, and nothing should be painted. */
export const SUPERSEDED = null;

/** A slice of the book's pages: `count` of them, starting at `first`. */
export interface Range {
  /** The first page to send, counting from 0. */
  first: number;
  /** How many pages to send. */
  count: number;
}

/**
 * A client over one worker.
 *
 * Every render raises the generation, so a reply that arrives behind
 * the current one resolves to `null` instead of bytes. A caller that
 * paints what it is given therefore cannot paint a stale page.
 */
export class Client {
  private readonly transport: Transport;
  private readonly waiting = new Map<number, (response: Response) => void>();
  private id = 0;
  private generation = 0;
  private counters: [number, number, number, number] = [0, 0, 0, 0];

  constructor(transport: Transport) {
    this.transport = transport;
  }

  /** Hands a reply from the worker to whoever is waiting for it. */
  receive(response: Response): void {
    const settle = this.waiting.get(response.id);
    if (settle === undefined) {
      return;
    }
    this.waiting.delete(response.id);
    settle(response);
  }

  /** The current generation. Raised only by a render whose `ops` is non-empty. */
  get current(): number {
    return this.generation;
  }

  /**
   * The generation a call to {@link Client.render}/{@link Client.preview}
   * with these `ops` will be answered under, without sending anything.
   * A caller that tags what it fetches with a generation of its own
   * asks here rather than mirror the rule this raises it by.
   */
  generationFor(ops: Op[]): number {
    return ops.length > 0 ? this.generation + 1 : this.generation;
  }

  /**
   * What the last painted render cost, counted in stage runs rather
   * than milliseconds: a cache that served shows here, where a clock
   * would only show a fast machine.
   */
  get stages(): { style: number; lines: number; flow: number; paint: number } {
    const [style, lines, flow, paint] = this.counters;
    return { style, lines, flow, paint };
  }

  /**
   * Applies inputs and asks for a display structure, `range` naming
   * which pages of it rather than the whole book. Resolves to `null`
   * when a later render overtook this one, or when its reply came
   * back behind the current generation.
   *
   * A ranged request (both `ops` empty and `range` given) is a
   * question about the book as it stands, not a render: within the
   * generation it was asked under, it never overtakes, and is never
   * overtaken by, another render or range. An edit still raises the
   * generation, and a range asked for before one arrives resolves to
   * `null` once it lands, the same as any other stale reply.
   */
  async preview(ops: Op[] = [], range?: Range): Promise<LayoutOutput | null> {
    const bytes = await this.render(ops, 'preview', range);
    return bytes === SUPERSEDED ? SUPERSEDED : decodeDisplayList(bytes);
  }

  /** The same, as PDF bytes. */
  async exportPdf(ops: Op[] = []): Promise<Uint8Array | null> {
    return this.render(ops, 'pdf');
  }

  /**
   * The file a face was registered from, for a painter that has to
   * draw with the bytes the engine shaped with.
   *
   * A question rather than a render: nothing overtakes it, and the
   * answer does not go stale, since a face keeps its id for the
   * session's life.
   */
  async fontBytes(font: number): Promise<Uint8Array> {
    const response = await this.send({ ops: [], want: 'font', font });
    if (!isRendered(response)) {
      throw new Error(`the engine sent no bytes for font ${font}`);
    }
    return response.bytes;
  }

  /**
   * The node one byte of one source was read into: the first step of
   * a cursor's way onto a page, since the node it answers with is the
   * one the display structure's runs name. `null` where nothing was
   * read: a blank line between chapters, or a file the book has not
   * read.
   *
   * A question rather than a render: nothing overtakes it. The answer
   * is about the book as the worker holds it, so an edit still in
   * flight is an edit this has not seen.
   */
  async nodeAt(source: string, byte: number): Promise<number | null> {
    return this.ask<number | null>({ ops: [], want: 'node', source, byte });
  }

  /**
   * The source a node was read from, and the bytes of it: the way
   * back, for a run under the pointer. `null` for a node the engine
   * synthesized, or one from a tree the host built rather than
   * parsed.
   */
  async sourceOf(node: number): Promise<NodeSource | null> {
    return this.ask<NodeSource | null>({ ops: [], want: 'source', node });
  }

  /**
   * Where each of these nodes' content is set: one answer per node,
   * in the order asked about. `null` for a node the book does not
   * hold, one the engine synthesized, or one whose content reaches
   * no page.
   *
   * This is the direction a reflow invalidates. A new face
   * repaginates the book, and a host that puts the reader back where
   * they were, or turns to a chapter, or names the chapter on
   * screen, asks where a node went. No page comes back with the
   * answer.
   *
   * The answer names the folios to print and the pages to fetch,
   * which a restarted page counter pulls apart: see {@link Folios}.
   *
   * A node covers itself and everything under it, so a heading
   * answers with the page its own text is on, and a chapter with the
   * pages it runs across.
   */
  async foliosOf(nodes: number[]): Promise<(Folios | null)[]> {
    return this.ask<(Folios | null)[]>({ ops: [], want: 'folios', nodes });
  }

  /** Applies inputs and asks for nothing back. */
  async apply(ops: Op[]): Promise<void> {
    await this.send({ ops });
  }

  /**
   * Applies inputs and asks for bytes: the display structure as the
   * engine encoded it, or a PDF. `null` when this render was
   * overtaken.
   *
   * The generation only raises when `ops` changed something. A range
   * fetch with nothing to apply asks about the current generation
   * rather than opening a new one, so it cannot be outrun by, or
   * outrun, a sibling range fetch that shares it — only a real edit
   * moves the generation such requests are answering against.
   */
  async render(ops: Op[], want: Want, range?: Range): Promise<Uint8Array | null> {
    this.generation = this.generationFor(ops);
    const response = await this.send({ ops, want, generation: this.generation, ...range });
    if (!isRendered(response)) {
      return SUPERSEDED;
    }
    // Only overtaking among the requests the worker already had shows
    // up there. A reply can also be outrun in flight, which only the
    // host can see.
    if (response.generation < this.generation) {
      return SUPERSEDED;
    }
    this.counters = response.stages;
    return response.bytes;
  }

  /** One question, and the JSON the worker answered it with. */
  private async ask<T>(what: {
    ops: Op[];
    want: Want;
    source?: string;
    byte?: number;
    node?: number;
    nodes?: number[];
  }): Promise<T> {
    const response = await this.send(what);
    if (!isRendered(response)) {
      throw new Error(`the engine answered no ${what.want}`);
    }
    return JSON.parse(new TextDecoder().decode(response.bytes)) as T;
  }

  private send(what: {
    ops: Op[];
    want?: Want;
    generation?: number;
    font?: number;
    source?: string;
    byte?: number;
    node?: number;
    nodes?: number[];
    first?: number;
    count?: number;
  }): Promise<Response> {
    this.id += 1;
    const request: Request = {
      id: this.id,
      generation: what.generation ?? this.generation,
      ops: what.ops,
      ...(what.want === undefined ? {} : { want: what.want }),
      ...(what.font === undefined ? {} : { font: what.font }),
      ...(what.source === undefined ? {} : { source: what.source }),
      ...(what.byte === undefined ? {} : { byte: what.byte }),
      ...(what.node === undefined ? {} : { node: what.node }),
      ...(what.nodes === undefined ? {} : { nodes: what.nodes }),
      ...(what.first === undefined ? {} : { first: what.first }),
      ...(what.count === undefined ? {} : { count: what.count }),
    };
    const transfer = request.ops
      .filter((op) => op.op === 'font' || op.op === 'image')
      .map((op) => op.bytes.buffer as ArrayBuffer);
    return new Promise<Response>((resolve, reject) => {
      this.waiting.set(request.id, (response) => {
        if (isFailed(response)) {
          reject(new Error(response.error));
        } else {
          resolve(response);
        }
      });
      this.transport.post(request, transfer);
    });
  }
}
