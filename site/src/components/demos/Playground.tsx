/**
 * The split view: a manuscript on the left, the book it sets on the
 * right, and the stylesheet between them when you want it.
 *
 * Every keystroke is an edit sent to the worker, and every reply
 * that has not been typed past is painted. Nothing is on a timer but
 * the editor: the engine is asked a fifth of a second after the
 * typing stops, and a render the reader has already outrun paints
 * nothing.
 */

import { useEffect, useRef, useState } from 'react';

import { read, write } from '../../demos/link';
import { usePreview } from '../../demos/usePreview';
import { Sheet } from './Sheet';

/** What the split view opens with. */
export interface PlaygroundProps {
  /** What this demo is called on the console registry. */
  id?: string;
  /** The manuscript it opens with. */
  markdown: string;
  /** The stylesheet it opens with. */
  css: string;
  /** What the manuscript is called. */
  name?: string;
  /** A page the engine painted at build time, as SVG markup. */
  poster: string;
  /** Whether the stylesheet editor is open to begin with. */
  stylesheet?: boolean;
  /** Whether to wait for a press before fetching the module. */
  held?: boolean;
}

/** How long after the last keystroke the engine is asked. */
const SETTLE = 200;

export function Playground(props: PlaygroundProps): React.ReactElement {
  const { id = 'playground', markdown: seed, css: sheet, name, poster, held } = props;

  const [draft, setDraft] = useState(seed);
  const [style, setStyle] = useState(sheet);
  const [markdown, setMarkdown] = useState(seed);
  const [css, setCss] = useState(sheet);
  const [stylesheet, setStylesheet] = useState(props.stylesheet ?? true);
  const [pane, setPane] = useState<'markdown' | 'css'>('markdown');
  const [page, setPage] = useState(1);
  const [settings, setSettings] = useState(false);
  const [copied, setCopied] = useState(false);
  const opened = useRef(false);

  const { sheet: frame, status, hydrated, error, output, start } = usePreview({
    id,
    markdown,
    css,
    name,
    page,
    held,
  });
  const pages = output?.pages.length ?? 0;
  const warnings = output?.warnings ?? [];

  // What was shared, if this page was opened through a link. Read
  // after the first paint, so the server's markup and the client's
  // first render are the same one.
  useEffect(() => {
    const shared = read();
    opened.current = true;
    if (shared === null) {
      return;
    }
    if (shared.markdown !== undefined) {
      setDraft(shared.markdown);
      setMarkdown(shared.markdown);
    }
    if (shared.css !== undefined) {
      setStyle(shared.css);
      setCss(shared.css);
    }
    if (shared.stylesheet !== undefined) {
      setStylesheet(shared.stylesheet);
    }
    if (shared.page !== undefined) {
      setPage(shared.page);
    }
  }, []);

  // The engine is asked once the typing stops. A keystroke that
  // lands inside the window replaces the one before it, so a
  // paragraph typed straight through costs one layout.
  useEffect(() => {
    const timer = setTimeout(() => setMarkdown(draft), SETTLE);
    return () => clearTimeout(timer);
  }, [draft]);

  useEffect(() => {
    const timer = setTimeout(() => setCss(style), SETTLE);
    return () => clearTimeout(timer);
  }, [style]);

  useEffect(() => {
    if (!opened.current) {
      return;
    }
    write({
      ...(markdown === seed ? {} : { markdown }),
      ...(css === sheet ? {} : { css }),
      ...(page === 1 ? {} : { page }),
      ...(stylesheet === (props.stylesheet ?? true) ? {} : { stylesheet }),
    });
  }, [markdown, css, page, stylesheet]);

  // A shorter book than the one that was on screen: the page being
  // read may no longer exist.
  useEffect(() => {
    if (pages > 0 && page > pages) {
      setPage(pages);
    }
  }, [pages]);

  useEffect(() => {
    if (!stylesheet && pane === 'css') {
      setPane('markdown');
    }
  }, [stylesheet]);

  function share(): void {
    void navigator.clipboard?.writeText(globalThis.location.href).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    });
  }

  return (
    <div className="d-split">
      <div className="d-edit">
        <div className="d-bar">
          <div className="d-tabs" role="tablist" aria-label="What to edit">
            <button
              type="button"
              role="tab"
              aria-selected={pane === 'markdown'}
              className="d-tab"
              onClick={() => setPane('markdown')}
            >
              Manuscript
            </button>
            {stylesheet && (
              <button
                type="button"
                role="tab"
                aria-selected={pane === 'css'}
                className="d-tab"
                onClick={() => setPane('css')}
              >
                Stylesheet
              </button>
            )}
          </div>
          <button
            type="button"
            className="d-icon"
            aria-expanded={settings}
            aria-label="Settings"
            onClick={() => setSettings(!settings)}
          >
            <span aria-hidden="true">⚙</span>
          </button>
        </div>

        {settings && (
          <div className="d-settings">
            <label>
              <input
                type="checkbox"
                checked={stylesheet}
                onChange={(event) => setStylesheet(event.target.checked)}
              />
              Edit the stylesheet
            </label>
            <button type="button" className="d-plain" onClick={share}>
              {copied ? 'Link copied' : 'Copy a link to this'}
            </button>
            <button
              type="button"
              className="d-plain"
              onClick={() => {
                setDraft(seed);
                setStyle(sheet);
              }}
            >
              Put it back
            </button>
          </div>
        )}

        <label className="d-hidden" htmlFor={`${id}-editor`}>
          {pane === 'markdown' ? 'Manuscript, as markdown' : 'Stylesheet, as CSS'}
        </label>
        <textarea
          id={`${id}-editor`}
          className="d-area"
          spellCheck={false}
          value={pane === 'markdown' ? draft : style}
          onChange={(event) =>
            pane === 'markdown' ? setDraft(event.target.value) : setStyle(event.target.value)
          }
        />
      </div>

      <div className="d-view">
        <Sheet
            sheet={frame}
            poster={poster}
            status={status}
            hydrated={hydrated}
            onStart={start}
            error={error}
          />
        <div className="d-pager">
          <button
            type="button"
            className="d-icon"
            onClick={() => setPage(Math.max(page - 1, 1))}
            disabled={page <= 1}
            aria-label="Previous page"
          >
            <span aria-hidden="true">‹</span>
          </button>
          <span className="d-folio" aria-live="polite">
            {pages === 0 ? 'page 1' : `page ${page} of ${pages}`}
          </span>
          <button
            type="button"
            className="d-icon"
            onClick={() => setPage(Math.min(page + 1, Math.max(pages, 1)))}
            disabled={pages === 0 || page >= pages}
            aria-label="Next page"
          >
            <span aria-hidden="true">›</span>
          </button>
        </div>
        {warnings.length > 0 && (
          <details className="d-warnings">
            <summary>
              {warnings.length} warning{warnings.length === 1 ? '' : 's'}
            </summary>
            <ul>
              {warnings.map((warning, at) => (
                <li key={at}>
                  {warning.origin === null ? '' : <code>{warning.origin}</code>} {warning.message}
                </li>
              ))}
            </ul>
          </details>
        )}
      </div>
    </div>
  );
}
