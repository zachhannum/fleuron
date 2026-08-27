/**
 * The split view: a manuscript on the left, the book it sets on the
 * right, and the stylesheet between them when you want it.
 *
 * Every keystroke is an edit sent to the worker, and every reply
 * that has not been typed past is painted. Nothing is on a timer but
 * the editor: the engine is asked a fifth of a second after the
 * typing stops, and a render the reader has already outrun paints
 * nothing.
 *
 * `editors` is what the prose around a demo is about. A page on the
 * CSS subset opens the stylesheet and nothing else; a page on the
 * markdown mapping opens the manuscript.
 */

import { useEffect, useRef, useState } from 'react';

import { read, write } from '../../demos/link';
import { usePreview } from '../../demos/usePreview';
import { Marks } from './Marks';
import { Sheet } from './Sheet';

/** Which editors a demo puts on screen. */
export type Editors = 'markdown' | 'css' | 'both';

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
  /** A page the engine painted at build time. */
  poster?: React.ReactNode;
  /** The same, when it arrives as the island's slot. */
  children?: React.ReactNode;
  /** Which editors are on screen. */
  editors?: Editors;
  /** The page to open on, counting from 1. */
  page?: number;
  /** Which markdown the manuscript is written in. */
  dialect?: 'commonmark' | 'gfm' | 'obsidian';
  /** What the display list is drawn as over the page it made. */
  highlight?: string;
  /** Whether the warnings are open to begin with. */
  warnings?: boolean;
  /** What the demo is, in one line, under it. */
  caption?: string;
}

/** How long after the last keystroke the engine is asked. */
const SETTLE = 200;

export function Playground(props: PlaygroundProps): React.ReactElement {
  const {
    id = 'playground',
    markdown: seed,
    css: sheet,
    name,
    dialect,
    highlight,
    caption,
  } = props;
  const poster = props.poster ?? props.children;
  // What the page asked for. The reader may close the stylesheet on
  // a demo that opened both, and may not open one the page did not.
  const asked = props.editors ?? 'both';
  const first = props.page ?? 1;

  const [draft, setDraft] = useState(seed);
  const [style, setStyle] = useState(sheet);
  const [markdown, setMarkdown] = useState(seed);
  const [css, setCss] = useState(sheet);
  const [editors, setEditors] = useState<Editors>(asked);
  const [pane, setPane] = useState<'markdown' | 'css'>(
    asked === 'css' ? 'css' : 'markdown',
  );
  const [page, setPage] = useState(first);
  const [settings, setSettings] = useState(false);
  const [copied, setCopied] = useState(false);
  const opened = useRef(false);

  const { sheet: frame, status, hydrated, error, output, start } = usePreview({
    id,
    markdown,
    css,
    name,
    page,
    dialect,
  });
  const pages = output?.pages.length ?? 0;
  const warnings = output?.warnings ?? [];
  const showing = output?.pages[Math.min(page, Math.max(pages, 1)) - 1];
  const shows = (which: 'markdown' | 'css'): boolean =>
    editors === which || editors === 'both';

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
    if (shared.stylesheet !== undefined && asked === 'both') {
      setEditors(shared.stylesheet ? 'both' : 'markdown');
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
      ...(page === first ? {} : { page }),
      ...(editors === asked ? {} : { stylesheet: editors === 'both' }),
    });
  }, [markdown, css, page, editors]);

  // A shorter book than the one that was on screen: the page being
  // read may no longer exist.
  useEffect(() => {
    if (pages > 0 && page > pages) {
      setPage(pages);
    }
  }, [pages]);

  useEffect(() => {
    if (!shows(pane)) {
      setPane(editors === 'css' ? 'css' : 'markdown');
    }
  }, [editors]);

  function share(): void {
    void navigator.clipboard?.writeText(globalThis.location.href).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    });
  }

  const split = (
    <div className="d-split">
      <div className="d-edit">
        <div className="d-bar">
          <div className="d-tabs" role="tablist" aria-label="What to edit">
            {shows('markdown') && (
              <button
                type="button"
                role="tab"
                aria-selected={pane === 'markdown'}
                className="d-tab"
                onClick={() => setPane('markdown')}
              >
                Manuscript
              </button>
            )}
            {shows('css') && (
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
            {asked === 'both' && (
              <label>
                <input
                  type="checkbox"
                  checked={editors === 'both'}
                  onChange={(event) =>
                    setEditors(event.target.checked ? 'both' : 'markdown')
                  }
                />
                Edit the stylesheet
              </label>
            )}
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
            marks={
              showing !== undefined && highlight !== undefined ? (
                <Marks page={showing} highlight={highlight} />
              ) : null
            }
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
            {pages === 0 ? `page ${page}` : `page ${page} of ${pages}`}
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
          <details className="d-warnings" open={props.warnings ?? false}>
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

  if (caption === undefined) {
    return split;
  }
  return (
    <figure className="d-demo">
      {split}
      <figcaption>{caption}</figcaption>
    </figure>
  );
}
