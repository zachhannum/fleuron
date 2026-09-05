/**
 * The stylesheets the islands ship with.
 *
 * These reach the browser, so they live apart from the catalogue,
 * which reads the corpus off disk and never leaves the server.
 */

/** What the bench sets its book in: a trade paperback with a folio. */
export const BENCH_CSS = `@page {
  size: 5.5in 8.5in;
  margin: 0.7in 0.6in 0.8in 0.7in;

  @bottom-center {
    content: counter(page);
    font-size: 9pt;
  }
}

book {
  font-size: 11pt;
  line-height: 1.4;
  text-align: justify;
  hyphens: auto;
}

section { break-before: recto; }

p {
  margin: 0;
  text-indent: 1.15em;
  orphans: 2;
  widows: 2;
}
`;

/**
 * The same sheet with one declaration changed.
 *
 * `line-height` is the expensive kind: it moves every baseline, so
 * the row it times is a real re-fragmentation rather than a cache
 * answering.
 */
export const RESTYLE_CSS = BENCH_CSS.replace('line-height: 1.4;', 'line-height: 1.5;');

/** A short manuscript for a demo that is about the stylesheet. */
export const SPECIMEN = `## A Chapter

It is a truth universally acknowledged, that a single man in
possession of a good fortune, must be in want of a wife.

However little known the feelings or views of such a man may be on his
first entering a neighbourhood, this truth is so well fixed in the
minds of the surrounding families, that he is considered the rightful
property of some one or other of their daughters.
`;

/** A stylesheet with two declarations the subset has no room for. */
export const OUTSIDE = `@page {
  size: 5.5in 8.5in;
  margin: 0.7in;
}

p {
  text-indent: 1.2em;
  color: crimson;
  border-bottom: 1px solid black;
}

blockquote {
  float: left;
}
`;
