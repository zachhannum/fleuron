/**
 * One page of a book, laid out here rather than photographed
 * somewhere else.
 *
 * Prose that says what the engine does with something can point at
 * the page it did it to, and the page is the engine's own output at
 * the moment it is read. A picture of a page can go stale against
 * the engine; this cannot.
 *
 * `highlight` draws the display list over the page it produced:
 * `baselines` puts a rule on every run's `y`, and `glyphs:N` ticks
 * the Nth run at each `x` the shaper gave it.
 */

import { usePreview } from '../../demos/usePreview';
import { Sheet } from './Sheet';

/** What one page is shown from. */
export interface PageViewProps {
  /** What this demo is called on the console registry. */
  id: string;
  /** The manuscript. */
  markdown: string;
  /** The stylesheet. */
  css: string;
  /** What the manuscript is called. */
  name?: string;
  /** The page to show, counting from 1. */
  page?: number;
  /** A page the engine painted at build time. */
  poster?: React.ReactNode;
  /** The same, when it arrives as the island's slot. */
  children?: React.ReactNode;
  /** What the prose is pointing at. */
  highlight?: string;
  /** What the page is, in one line, under it. */
  caption?: string;
}

export function PageView(props: PageViewProps): React.ReactElement {
  const { id, markdown, css, name, page = 1, highlight, caption } = props;
  const poster = props.poster ?? props.children;
  const { sheet, status, hydrated, error, output, start } = usePreview({
    id,
    markdown,
    css,
    name,
    page,
    held: true,
  });
  const showing = output?.pages[Math.min(page, output.pages.length) - 1];

  return (
    <figure className="d-figure">
      <div className="d-view">
        <Sheet
          sheet={sheet}
          poster={poster}
          status={status}
          hydrated={hydrated}
          onStart={start}
          error={error}
        />
        {showing !== undefined && highlight !== undefined && (
          <svg
            className="d-marks"
            viewBox={`0 0 ${showing.width} ${showing.height}`}
            aria-hidden="true"
          >
            {marks(showing, highlight)}
          </svg>
        )}
      </div>
      {caption !== undefined && (
        <figcaption>
          {caption}
          {hydrated && status !== 'live' && ' Press the button to lay it out here.'}
        </figcaption>
      )}
    </figure>
  );
}

/** What the display list says, drawn over the page it made. */
function marks(
  page: { items: { kind: string }[] },
  highlight: string,
): React.ReactElement[] {
  const runs = page.items.filter(
    (item): item is { kind: 'text'; x: number; y: number; size: number; glyphs: { x: number }[] } =>
      item.kind === 'text',
  );
  if (highlight === 'baselines') {
    return runs.map((run, at) => (
      <line
        key={at}
        x1={run.x}
        y1={run.y}
        x2={(run.glyphs[run.glyphs.length - 1]?.x ?? run.x) + run.size * 0.5}
        y2={run.y}
        className="d-mark-line"
      />
    ));
  }
  const [what, which] = highlight.split(':');
  if (what === 'glyphs') {
    const run = runs[Number(which ?? 0)];
    return run === undefined
      ? []
      : run.glyphs.map((glyph, at) => (
          <line
            key={at}
            x1={glyph.x}
            y1={run.y - run.size * 0.72}
            x2={glyph.x}
            y2={run.y + run.size * 0.24}
            className="d-mark-tick"
          />
        ));
  }
  return [];
}
