/**
 * What the display list says, drawn over the page it made.
 *
 * `baselines` puts a rule on every run's `y`, and `glyphs:N` ticks
 * the Nth run at each `x` the shaper gave it. Prose about a field
 * points at the page that field placed.
 */

/** A page of the display list, and what to draw over it. */
export interface MarksProps {
  /** The page the marks are taken from. */
  page: { width: number; height: number; items: { kind: string }[] };
  /** What the prose is pointing at: `baselines`, or `glyphs:N`. */
  highlight: string;
}

export function Marks(props: MarksProps): React.ReactElement {
  const { page, highlight } = props;
  return (
    <svg className="d-marks" viewBox={`0 0 ${page.width} ${page.height}`} aria-hidden="true">
      {marks(page, highlight)}
    </svg>
  );
}

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
