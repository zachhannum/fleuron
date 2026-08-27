/**
 * The one bright object: a page, on a dark surface.
 *
 * Two layers sit in the same box. The poster is a real page the
 * engine painted at build time and is what the server sends, so a
 * reader whose JavaScript never runs still sees a typeset page. The
 * live layer is what the worker paints into, and it takes over the
 * moment the first display list arrives.
 */

import type { Status } from '../../demos/usePreview';

/** What a sheet is given. */
export interface SheetProps {
  /** The element the preview paints into. */
  sheet: React.RefObject<HTMLDivElement | null>;
  /**
   * A page the engine painted at build time.
   *
   * Handed down as markup the server rendered rather than as a
   * string in the island's props: a prop is serialised into the page
   * next to the DOM it produced, and a page carrying a poster twice
   * pays for it twice.
   */
  poster: React.ReactNode;
  status: Status;
  /** Whether the island is running yet. */
  hydrated: boolean;
  /** Fetches the module, for a demo that is waiting to be asked. */
  onStart: () => void;
  /** What broke, when something did. */
  error?: string | null;
  /** The display list, drawn over the page it made. */
  marks?: React.ReactNode;
  /** The size of the module, for a reader deciding whether to spend it. */
  weight?: string;
}

export function Sheet(props: SheetProps): React.ReactElement {
  const { sheet, poster, status, hydrated, onStart, error, marks, weight = '3.7 MB' } = props;
  return (
    <div className="d-sheet" data-status={status}>
      <div className="d-paper d-poster" aria-hidden={status === 'live'}>
        {poster}
      </div>
      <div className="d-paper d-live" ref={sheet} />
      {marks}
      {hydrated && status === 'held' && (
        <div className="d-veil">
          <button type="button" className="d-run" onClick={onStart}>
            Run the engine here
          </button>
          <p>{weight} of engine, fetched once. The book is laid out here.</p>
        </div>
      )}
      {hydrated && status === 'loading' && (
        <div className="d-veil d-veil-quiet">
          <p>
            <span className="d-pulse" aria-hidden="true" /> Fetching the engine
          </p>
        </div>
      )}
      {hydrated && status === 'broken' && (
        <div className="d-veil">
          <p className="d-broke">The engine stopped: {error}</p>
        </div>
      )}
    </div>
  );
}
