/**
 * The worker's side of the wall: the module, the session it keeps,
 * and the rule that only the newest render is worth running.
 *
 * Requests are applied in the order they arrive and renders are not.
 * A render that a later request overtakes before it starts is
 * dropped, and the inputs it carried are applied all the same. So a
 * dropped render leaves no stage half-built, and the render that
 * follows it produces exactly what it would have produced had nobody
 * typed.
 */

import init, { Session, type InitInput } from '../wasm/fleuron.js';
import type { Op, Request, Response } from './protocol.js';

/** How the module is loaded. */
export interface EngineOptions {
  /**
   * The module itself, or where to fetch it. A browser that serves
   * the package can leave this out and let the module find its own
   * `.wasm` beside the glue. Anywhere without `fetch` over the
   * package's own files (Node, an extension, a bundler that inlines
   * the module) passes the bytes.
   */
  wasm?: InitInput;
}

/** Sends one reply, moving the bytes rather than copying them. */
export type Reply = (response: Response, transfer: ArrayBuffer[]) => void;

interface Pending {
  request: Request;
  reply: Reply;
}

/**
 * Loads the module and opens a session over it.
 *
 * One session per worker: it holds the manuscript, the styling and
 * every stage between them, which is what makes the second render of
 * a book cost what changed rather than the book.
 */
export async function createEngine(options: EngineOptions = {}): Promise<Engine> {
  await init(options.wasm === undefined ? undefined : { module_or_path: options.wasm });
  return new Engine(new Session());
}

/** A session, and the queue of requests waiting on it. */
export class Engine {
  private readonly session: Session;
  private readonly queue: Pending[] = [];
  private draining = false;

  constructor(session: Session) {
    this.session = session;
  }

  /** Takes a request. Replies arrive through `reply`, in order. */
  submit(request: Request, reply: Reply): void {
    this.queue.push({ request, reply });
    void this.drain();
  }

  /** Releases the module's session. */
  free(): void {
    this.session.free();
  }

  private async drain(): Promise<void> {
    if (this.draining) {
      return;
    }
    this.draining = true;
    try {
      while (this.queue.length > 0) {
        // Everything already sent is delivered before anything is
        // rendered, which is what makes latest-wins mean the latest
        // the host has actually asked for rather than the latest one
        // request ago.
        await settle();
        const batch = this.queue.splice(0);
        const newest = lastRenderIn(batch);
        batch.forEach((pending, index) => this.run(pending, index === newest));
      }
    } finally {
      this.draining = false;
    }
  }

  private run(pending: Pending, render: boolean): void {
    const { request, reply } = pending;
    const { id, generation } = request;
    try {
      // Inputs are applied whether or not this request's render
      // survives: an edit that crossed the wall is not undone by the
      // keystroke that followed it.
      for (const op of request.ops) {
        this.apply(op);
      }
      if (request.want === undefined) {
        reply({ id, generation, applied: true }, []);
        return;
      }
      if (!render && request.want !== 'font') {
        reply({ id, generation, superseded: true }, []);
        return;
      }
      const bytes = this.produce(request);
      const stages = this.session.stages();
      reply(
        {
          id,
          generation,
          kind: request.want,
          bytes,
          stages: [stages[0] ?? 0, stages[1] ?? 0, stages[2] ?? 0, stages[3] ?? 0],
        },
        [bytes.buffer as ArrayBuffer],
      );
    } catch (error) {
      reply({ id, generation, error: String(error) }, []);
    }
  }

  private produce(request: Request): Uint8Array {
    switch (request.want) {
      case 'pdf':
        return this.session.exportPdf();
      case 'font':
        return this.session.fontBytes(request.font ?? 0);
      default:
        return this.session.preview();
    }
  }

  private apply(op: Op): void {
    switch (op.op) {
      case 'font':
        this.session.addFont(op.bytes);
        break;
      case 'dialect':
        this.session.setDialect(op.dialect);
        break;
      case 'split':
        this.session.setSplit(op.level);
        break;
      case 'markdown':
        this.session.setMarkdown(op.name, op.text);
        break;
      case 'book':
        this.session.setSources(
          op.sources.map((source) => source.name),
          op.sources.map((source) => source.text),
        );
        break;
      case 'remove':
        this.session.removeMarkdown(op.name);
        break;
      case 'metadata':
        this.session.setMetadata(JSON.stringify(op.metadata));
        break;
      case 'edit':
        this.session.updateMarkdown(op.name, op.text);
        break;
      case 'content':
        this.session.setContent(op.json);
        break;
      case 'style':
        this.session.setStyle(op.css);
        break;
    }
  }
}

/**
 * Which request in a batch is the one whose render still matters.
 *
 * A question is not a render: asking for a face's bytes neither
 * overtakes a render nor is overtaken by one.
 */
function lastRenderIn(batch: Pending[]): number {
  for (let index = batch.length - 1; index >= 0; index -= 1) {
    const want = batch[index]?.request.want;
    if (want === 'preview' || want === 'pdf') {
      return index;
    }
  }
  return -1;
}

/** Yields long enough for messages already sent to be delivered. */
function settle(): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, 0);
  });
}
