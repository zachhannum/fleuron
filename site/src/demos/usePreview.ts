/**
 * The island's half of the wall: an element, the inputs, and a page
 * on screen.
 *
 * Everything about the engine lives in the worker. This holds a
 * `Preview` from `@fleuron/wasm`, hands it whatever prop changed,
 * and reports what came back. It knows the shape of a display list
 * and nothing about how one is made.
 *
 * The module is not fetched until a demo is on screen, so a visitor
 * who never scrolls to one downloads no engine. A visitor whose
 * browser says the connection is metered is asked first.
 */

import { Preview, type LayoutOutput } from '@fleuron/wasm';
import { useEffect, useRef, useState } from 'react';

import { spawn } from './spawn';

/** Where a demo is in its life. */
export type Status =
  /** Waiting to be asked on a metered connection: nothing has been fetched. */
  | 'held'
  /** The module is on its way. */
  | 'loading'
  /** A page is on screen. */
  | 'live'
  /** Something threw, and `error` says what. */
  | 'broken';

/**
 * The faces the site serves, under the path the catalogue names.
 * The painter resolves a run against the document, so the file is
 * already on the page and the island fetches that copy rather than
 * a second one.
 */
const served: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob('../../../fixtures/fonts/*.ttf', {
      query: '?url',
      import: 'default',
      eager: true,
    }),
  ).map(([path, url]) => [path.replace('../../../fixtures/', ''), url as string]),
);

/** One face a demo's stylesheet names, and where its file is. */
export interface Face {
  /** The file, relative to `fixtures/`. */
  url: string;
  /** The family the engine reads out of it, which CSS selects by. */
  family: string;
  /** The weight it registers at. */
  weight: number;
}

/** What a demo runs. */
export interface Inputs {
  /** What this demo is called on the console registry. */
  id: string;
  /** The manuscript. */
  markdown: string;
  /** The author stylesheet. */
  css: string;
  /** What the manuscript is called, which is what an edit replaces. */
  name?: string;
  /** The page to show, counting from 1. */
  page?: number;
  /** Which markdown the manuscript is written in. */
  dialect?: 'commonmark' | 'gfm' | 'obsidian';
  /**
   * The images the manuscript refers to, by the url it names them
   * by. Nothing fetches a url for the island, so it fetches each one
   * from where the site serves it and hands over the bytes.
   */
  images?: string[];
  /**
   * The faces the stylesheet names beyond the bundled one, fetched
   * the same way and registered for the session's life.
   */
  fonts?: Face[];
}

/** A demo, mounted. */
export interface Running {
  /** The element the preview paints into. */
  sheet: React.RefObject<HTMLDivElement | null>;
  status: Status;
  /** What broke, when something did. */
  error: string | null;
  /** The last render that reached the screen. */
  output: LayoutOutput | null;
  /**
   * Whether the island is running yet.
   *
   * What the server sends is a poster and nothing else: a button
   * that cannot be pressed, or a note about a fetch that is not
   * happening, is worse than no chrome at all to a reader whose
   * scripts never run.
   */
  hydrated: boolean;
  /** Fetches the module and mounts, for a demo a metered browser held. */
  start: () => void;
}

/**
 * Every mounted demo, by id, for a console and for the browser check
 * that holds an island's SVG against the display list behind it.
 */
declare global {
  // eslint-disable-next-line no-var
  var fleuron: Map<string, { preview: Preview; output: LayoutOutput | null }> | undefined;
}

function register(id: string, preview: Preview): void {
  globalThis.fleuron ??= new Map();
  globalThis.fleuron.set(id, { preview, output: null });
}

function recorded(id: string, output: LayoutOutput): void {
  const entry = globalThis.fleuron?.get(id);
  if (entry !== undefined) {
    entry.output = output;
  }
}

/**
 * Runs once the page has finished loading and the browser has
 * nothing better to do.
 *
 * The module is megabytes. A fetch that starts while the page is
 * still painting takes the bandwidth the page is painting with, and
 * the reader waits for a book they have not asked to see yet.
 */
function whenIdle(run: () => void): () => void {
  let cancelled = false;
  const soon = (): void => {
    if (cancelled) {
      return;
    }
    const go = (): void => {
      if (!cancelled) {
        run();
      }
    };
    if (typeof requestIdleCallback === 'function') {
      requestIdleCallback(go, { timeout: 2000 });
    } else {
      setTimeout(go, 200);
    }
  };
  if (document.readyState === 'complete') {
    soon();
  } else {
    addEventListener('load', soon, { once: true });
  }
  return () => {
    cancelled = true;
  };
}

/** Whether the reader has asked their browser to spend less. */
function metered(): boolean {
  const connection = (
    navigator as Navigator & { connection?: { saveData?: boolean; effectiveType?: string } }
  ).connection;
  return (
    connection?.saveData === true ||
    connection?.effectiveType === 'slow-2g' ||
    connection?.effectiveType === '2g'
  );
}

export function usePreview(inputs: Inputs): Running {
  const { id, markdown, css, name, page, dialect, images, fonts } = inputs;
  const sheet = useRef<HTMLDivElement | null>(null);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [wanted, setWanted] = useState(true);
  const [status, setStatus] = useState<Status>('loading');
  const [error, setError] = useState<string | null>(null);
  const [output, setOutput] = useState<LayoutOutput | null>(null);
  const [hydrated, setHydrated] = useState(false);

  useEffect(() => setHydrated(true), []);

  // The reader's own setting outranks the demo: a demo waits for a
  // press on a connection the browser says is metered.
  useEffect(() => {
    if (wanted && metered()) {
      setWanted(false);
      setStatus('held');
    }
  }, []);

  useEffect(() => {
    if (!wanted) {
      return;
    }
    let live = true;
    let opened: Preview | null = null;
    let worker: Worker | null = null;
    const cancel = whenIdle(() => {
      worker = spawn();
      open(worker);
    });

    const open = (opening: Worker): void => {
      void Preview.mount(sheet.current as Element, {
        worker: opening,
        dialect,
        // The site already serves the face the engine shaped with,
        // subset from the same file, so the painter's stack resolves
        // to it and no book face crosses back over the wall.
        faces: 'host',
        paper: null,
        ink: 'currentColor',
        onRender: (rendered) => {
          recorded(id, rendered);
          setOutput(rendered);
          setStatus('live');
        },
      })
        .then(async (mounted) => {
          opened = mounted;
          if (!live) {
            mounted.destroy();
            return;
          }
          // The faces cross before the stylesheet that names them,
          // and the images before the manuscript, so the first page
          // painted is set in the right face with room for them.
          for (const face of fonts ?? []) {
            const at = served[face.url] ?? `${import.meta.env.BASE_URL}fixtures/${face.url}`;
            const file = await fetch(at);
            if (!live) {
              return;
            }
            await mounted.addFont(new Uint8Array(await file.arrayBuffer()));
          }
          for (const url of images ?? []) {
            const file = await fetch(`${import.meta.env.BASE_URL}fixtures/${url}`);
            if (!live) {
              return;
            }
            await mounted.addImage(url, new Uint8Array(await file.arrayBuffer()));
          }
          if (!live) {
            return;
          }
          register(id, mounted);
          setPreview(mounted);
        })
        .catch((thrown: unknown) => {
          if (live) {
            setError(String(thrown));
            setStatus('broken');
          }
        });
    };

    return () => {
      live = false;
      cancel();
      opened?.destroy();
      worker?.terminate();
      globalThis.fleuron?.delete(id);
    };
  }, [wanted, id]);

  // The stylesheet reaches the engine before the manuscript, so the
  // first page painted is already the styled one.
  useEffect(() => {
    if (preview !== null) {
      run(() => preview.setStyle(css));
    }
  }, [preview, css]);

  useEffect(() => {
    if (preview !== null) {
      // The keystroke path: one source replaced, and the rest of
      // the book left standing. A name the book has not seen yet is
      // appended, so the first edit is also how it is opened.
      run(() => preview.edit(name ?? 'manuscript.md', markdown));
    }
  }, [preview, markdown, name]);

  useEffect(() => {
    if (preview !== null && page !== undefined) {
      preview.page = page;
    }
  }, [preview, page, output]);

  function run(call: () => Promise<void>): void {
    call().catch((thrown: unknown) => {
      setError(String(thrown));
      setStatus('broken');
    });
  }

  return {
    sheet,
    status,
    error,
    output,
    hydrated,
    start: () => {
      setWanted(true);
      setStatus('loading');
    },
  };
}
