/**
 * The perf table, computed rather than quoted.
 *
 * A whole novel, laid out in this browser, on this machine, with the
 * clock read on this side of the worker so that what is timed is
 * what a host would actually wait for. The machine is named next to
 * the numbers, because a number without one is marketing.
 *
 * Each row says which stages ran, taken from the engine's own
 * counters, so a row that reused a stage cannot pass itself off as a
 * row that ran it.
 */

import { Client, type Op } from 'fleuron';
import { useEffect, useRef, useState } from 'react';

import { BENCH_CSS, RESTYLE_CSS } from '../../demos/sheets';
import { spawn } from '../../demos/spawn';

/** What the bench is run over. */
export interface BenchProps {
  /** Where the book is served from. */
  book: string;
  /** What it is called. */
  title: string;
}

/** One thing that was timed. */
interface Row {
  what: string;
  ms: number;
  stages: string;
}

const NAMES = ['style', 'lines', 'flow', 'paint'] as const;

export function Bench(props: BenchProps): React.ReactElement {
  const { book, title } = props;
  const [rows, setRows] = useState<Row[]>([]);
  const [pages, setPages] = useState(0);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [where, setWhere] = useState('');
  const [hydrated, setHydrated] = useState(false);
  const worker = useRef<Worker | null>(null);

  // Read after mounting. The server has a machine of its own, and a
  // number measured here belongs next to the one it was measured on.
  // The button waits for the same moment: the markup the server
  // sends has no engine behind it.
  useEffect(() => {
    setWhere(machine());
    setHydrated(true);
  }, []);

  async function measure(): Promise<void> {
    setRunning(true);
    setError(null);
    setRows([]);
    try {
      const text = await (await fetch(book)).text();
      worker.current?.terminate();
      const spawned = spawn();
      worker.current = spawned;
      const client = new Client({
        post: (request, transfer) => spawned.postMessage(request, transfer),
      });
      spawned.addEventListener('message', (event: MessageEvent) => client.receive(event.data));

      const timed: Row[] = [];
      let before = client.stages;
      const time = async (what: string, run: () => Promise<unknown>): Promise<void> => {
        const start = performance.now();
        await run();
        const ms = performance.now() - start;
        const after = client.stages;
        timed.push({ what, ms, stages: ran(before, after) });
        before = after;
        setRows([...timed]);
      };

      // Nothing is asked for back, so what this waits on is the
      // module arriving and the session opening.
      await time('fetch and start the engine', () => client.apply([]));

      const setup: Op[] = [
        { op: 'style', css: BENCH_CSS },
        { op: 'markdown', name: 'book.md', text },
      ];
      await time(`lay ${title} out`, async () => {
        const output = await client.preview(setup);
        setPages(output?.pages.length ?? 0);
      });
      await time('change one declaration', () =>
        client.preview([{ op: 'style', css: RESTYLE_CSS }]),
      );
      await time('write the PDF', () => client.exportPdf());
      spawned.terminate();
      worker.current = null;
    } catch (thrown: unknown) {
      setError(String(thrown));
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="d-bench">
      <div className="d-bar">
        {hydrated && (
          <button type="button" className="d-run" onClick={() => void measure()} disabled={running}>
            {running ? 'Running' : rows.length > 0 ? 'Run it again' : 'Run it here'}
          </button>
        )}
        <span className="d-machine">{where}</span>
      </div>
      {error !== null && <p className="d-broke">The engine stopped: {error}</p>}
      {rows.length > 0 && (
        <table className="d-table">
          <caption className="d-hidden">
            What each part of laying out {title} cost in this browser.
          </caption>
          <thead>
            <tr>
              <th scope="col">what</th>
              <th scope="col">stages run</th>
              <th scope="col">wall clock</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.what}>
                <th scope="row">{row.what}</th>
                <td className="d-stages">{row.stages}</td>
                <td className="d-ms">{clock(row.ms)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {pages > 0 && (
        <p className="d-note">
          {title} set in {pages} pages. A row that ran no stages did no layout: the first is the
          network and the module starting, and the export paints the stages the layout already
          settled rather than setting the book twice.
        </p>
      )}
    </div>
  );
}

/** Which stages ran between two readings of the counters. */
function ran(before: Client['stages'], after: Client['stages']): string {
  const moved = NAMES.filter((name) => after[name] > before[name]);
  return moved.length === 0 ? 'none' : moved.join(' · ');
}

function clock(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${Math.round(ms)} ms`;
}

/** What the browser will say about the machine it is running on. */
function machine(): string {
  const nav = navigator as Navigator & {
    deviceMemory?: number;
    userAgentData?: { platform?: string };
  };
  const platform = nav.userAgentData?.platform ?? guessPlatform();
  const cores = nav.hardwareConcurrency;
  const memory = nav.deviceMemory;
  return [
    platform,
    cores === undefined ? null : `${cores} cores`,
    memory === undefined ? null : `${memory} GB`,
  ]
    .filter((part) => part !== null)
    .join(' · ');
}

function guessPlatform(): string {
  const agent = navigator.userAgent;
  const found = /\((?:[^;)]*;\s*)?([^;)]+)/.exec(agent);
  return found?.[1]?.trim() ?? 'this browser';
}
