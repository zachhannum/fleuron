/**
 * The host's side of the wall: send an edit, get a display structure, and
 * never paint one the reader has already typed past.
 */

import { isFailed, isRendered, type Op, type Request, type Response, type Want } from './protocol.js';
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

  /** The generation the next render goes out under. */
  get current(): number {
    return this.generation;
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
    if (ops.length > 0) {
      this.generation += 1;
    }
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

  private send(what: {
    ops: Op[];
    want?: Want;
    generation?: number;
    font?: number;
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
