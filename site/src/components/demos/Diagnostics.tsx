/**
 * The diagnostics channel, demonstrated rather than described.
 *
 * CSS in, warnings out, each naming the line and column it was
 * written at. A declaration outside the subset is reported and the
 * book lays out anyway, which is the whole claim: nothing is
 * silently dropped, and nothing stops the run.
 */

import { useEffect, useState } from 'react';

import { OUTSIDE, SPECIMEN } from '../../demos/sheets';
import { usePreview } from '../../demos/usePreview';

/** What the diagnostics demo opens with. */
export interface DiagnosticsProps {
  /** The stylesheet it opens with. */
  css?: string;
  /** A page the engine painted at build time, as SVG markup. */
  poster?: string;
}

const SETTLE = 200;

export function Diagnostics(props: DiagnosticsProps): React.ReactElement {
  const seed = props.css ?? OUTSIDE;
  const [style, setStyle] = useState(seed);
  const [css, setCss] = useState(seed);
  const { sheet, status, hydrated, output, error, start } = usePreview({
    id: 'diagnostics',
    markdown: SPECIMEN,
    css,
    name: 'specimen.md',
    held: true,
  });

  useEffect(() => {
    const timer = setTimeout(() => setCss(style), SETTLE);
    return () => clearTimeout(timer);
  }, [style]);

  const warnings = output?.warnings ?? [];

  return (
    <div className="d-diagnostics">
      <label className="d-hidden" htmlFor="diagnostics-css">
        A stylesheet
      </label>
      <textarea
        id="diagnostics-css"
        className="d-area"
        spellCheck={false}
        value={style}
        onChange={(event) => setStyle(event.target.value)}
      />
      <div className="d-report">
        {hydrated && status === 'held' && (
          <p className="d-note">
            <button type="button" className="d-run" onClick={start}>
              Compile it here
            </button>
          </p>
        )}
        {status === 'loading' && <p className="d-note">Fetching the engine.</p>}
        {status === 'broken' && <p className="d-broke">The engine stopped: {error}</p>}
        {status === 'live' && warnings.length === 0 && (
          <p className="d-note">Nothing to report: every declaration is in the subset.</p>
        )}
        {warnings.length > 0 && (
          <ul className="d-warnlist">
            {warnings.map((warning, at) => (
              <li key={at}>
                <code>{warning.origin ?? 'the book'}</code>
                <span>{warning.message}</span>
              </li>
            ))}
          </ul>
        )}
        {status === 'live' && (
          <p className="d-note">
            {output?.pages.length ?? 0} page{output?.pages.length === 1 ? '' : 's'}, laid out
            anyway. A warning is a book that set.
          </p>
        )}
      </div>
      {/* The engine paints somewhere. The page itself is not what this
          demo is about, so it is kept out of the way rather than shown. */}
      <div className="d-offstage" ref={sheet} aria-hidden="true" />
    </div>
  );
}
