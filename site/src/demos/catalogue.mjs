/**
 * What the demos run: a manuscript, a stylesheet, and the page to
 * open on.
 *
 * Read on the server. The poster generator opens these through the
 * engine at build time and the pages hand them to an island as
 * props, so a demo and its poster can never be set from different
 * text.
 */

import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

/**
 * The repository, found by walking up from wherever this was run.
 *
 * Not from `import.meta.url`: Astro bundles this file into its
 * prerender output, and a path relative to the module would then be
 * relative to the bundle rather than to the source.
 */
function repository() {
  let at = resolve(process.cwd());
  while (!existsSync(join(at, 'fixtures', 'corpus'))) {
    const up = dirname(at);
    if (up === at) {
      throw new Error('the fixture corpus is not above the working directory');
    }
    at = up;
  }
  return at;
}

const root = repository();

/** A fixture, read from the corpus the perf gate measures. */
export function fixture(path) {
  return readFileSync(join(root, 'fixtures', path), 'utf8');
}

/**
 * One chapter of a corpus book, without the frontmatter.
 *
 * A demo opens on the chapter rather than on a title page, which is
 * the page worth looking at and the one that shows a drop cap.
 */
export function chapter(path, number) {
  const text = fixture(path);
  const heads = [...text.matchAll(/^## .*$/gm)];
  const from = heads[number - 1];
  const to = heads[number];
  if (from?.index === undefined) {
    throw new Error(`${path} has no chapter ${number}`);
  }
  return text.slice(from.index, to?.index).trimEnd();
}

/**
 * The demo stylesheet: a trade paperback, mirrored margins, a
 * running head from the chapter title, and a three-line drop cap.
 * Every rule is in the subset, which is the point of showing it.
 */
export const BOOK_CSS = `@page {
  size: 5.5in 8.5in;
  margin: 0.7in 0.6in 0.8in 0.7in;

  @bottom-center {
    content: counter(page);
    font-size: 9pt;
  }
}

/* The wider margin is the spine on both sides of the spread. */
@page :left {
  margin-left: 0.6in;
  margin-right: 0.7in;

  @top-center {
    content: "Pride and Prejudice";
    font-size: 8pt;
  }
}

@page :right {
  @top-center {
    content: string(chapter);
    font-size: 8pt;
  }
}

/* A chapter's opening page carries no head. */
@page :first {
  @top-center { content: none; }
}

book {
  font-size: 11pt;
  line-height: 1.4;
  text-align: justify;
  hyphens: auto;
}

section {
  break-before: recto;
}

h2 {
  string-set: chapter content();
  font-size: 13pt;
  font-weight: 400;
  text-align: center;
  margin: 0 0 28pt;
}

p {
  margin: 0;
  text-indent: 1.15em;
  orphans: 2;
  widows: 2;
}

h2 + p {
  text-indent: 0;
}

h2 + p::first-letter {
  initial-letter: 3;
}
`;

/** The same book with nothing said about it: the built-in sheet alone. */
export const BARE_CSS = `@page {
  size: 5.5in 8.5in;
  margin: 0.7in 0.6in 0.8in 0.7in;
}
`;

/** A stylesheet with two declarations the subset has no room for. */
export const OUTSIDE_CSS = `@page {
  size: 5.5in 8.5in;
  margin: 0.7in;
}

p {
  text-indent: 1.2em;
  color: crimson;
  border-bottom: 1px solid black;
}
`;

/**
 * Every demo the site mounts, and the poster each one is drawn on
 * top of until it does.
 */
export const DEMOS = {
  /** The landing page's split view, and the docs' playground. */
  playground: () => ({
    name: 'pride-and-prejudice.md',
    markdown: chapter('corpus/pride-and-prejudice.md', 1),
    css: BOOK_CSS,
    page: 1,
  }),
  /** What the styled sheet does to a chapter opening. */
  'css-subset': () => ({
    name: 'pride-and-prejudice.md',
    markdown: chapter('corpus/pride-and-prejudice.md', 1),
    css: BOOK_CSS,
    page: 1,
  }),
  /** A page of running text, for pointing at one box on. */
  'display-list': () => ({
    name: 'pride-and-prejudice.md',
    markdown: chapter('corpus/pride-and-prejudice.md', 1),
    css: BOOK_CSS,
    page: 2,
  }),
  /** What the CLI quickstart writes, from the manuscript it writes it from. */
  quickstart: () => ({
    name: 'gulliver-excerpt.md',
    markdown: fixture('gulliver-excerpt.md'),
    css: fixture('styled.css'),
    page: 1,
  }),
  /** The bare sheet, for holding the styled one against. */
  bare: () => ({
    name: 'pride-and-prejudice.md',
    markdown: chapter('corpus/pride-and-prejudice.md', 1),
    css: BARE_CSS,
    page: 1,
  }),
};

/** One demo's inputs, by name. */
export function demo(id) {
  const build = DEMOS[id];
  if (build === undefined) {
    throw new Error(`no demo called ${id}`);
  }
  return build();
}
