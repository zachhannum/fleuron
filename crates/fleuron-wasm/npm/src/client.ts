/**
 * The host's side of the wall: send an edit, get a display list, and
 * never paint one the reader has already typed past.
 */

import { isFailed, isRendered, type Op, type Request, type Response, type Want } from './protocol.js';
import { decodeDisplayList, type LayoutOutput } from './wire.js';

/** How a request reaches the worker. */
export interface Transport {
  /**
   * Sends one request. `transfer` holds the buffers that should move
   * rather than be copied: font bytes, which the host has no reason
   * to keep a second copy of.
   */
  post(request: Request, transfer: ArrayBuffer[]): void;
}

/** A render that was overtaken: nothing came back, and nothing should be painted. */
export const SUPERSEDED = null;

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

  /** The generation the next render will carry. */
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
   * Applies inputs and asks for a display list. Resolves to `null`
   * when a later render overtook this one, or when its reply came
   * back behind the current generation.
   */
  async preview(ops: Op[] = []): Promise<LayoutOutput | null> {
    const bytes = await this.render(ops, 'preview');
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
   * Applies inputs and asks for bytes: the display list as the
   * engine encoded it, or a PDF. `null` when this render was
   * overtaken.
   */
  async render(ops: Op[], want: Want): Promise<Uint8Array | null> {
    this.generation += 1;
    const response = await this.send({ ops, want, generation: this.generation });
    if (!isRendered(response)) {
      return SUPERSEDED;
    }
    // The worker only knows it was overtaken by a request it had in
    // hand. A reply can also be outrun in flight, and the host is the
    // one who can see that.
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
  }): Promise<Response> {
    this.id += 1;
    const request: Request = {
      id: this.id,
      generation: what.generation ?? this.generation,
      ops: what.ops,
      ...(what.want === undefined ? {} : { want: what.want }),
      ...(what.font === undefined ? {} : { font: what.font }),
    };
    const transfer = request.ops
      .filter((op) => op.op === 'font')
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
