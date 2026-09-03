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

/** A fixture image or font file, as the bytes a host hands the engine. */
export function bytes(url) {
  return new Uint8Array(readFileSync(join(root, 'fixtures', url)));
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
 * A chapter with whatever stands above it: the frontmatter the
 * frontend reads a title from, and the image a book opens on.
 */
export function opening(path, number) {
  const text = fixture(path);
  const heads = [...text.matchAll(/^## .*$/gm)];
  if (heads[number - 1]?.index === undefined) {
    throw new Error(`${path} has no chapter ${number}`);
  }
  return text.slice(0, heads[number]?.index).trimEnd();
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

/**
 * The same paperback, opened on a book with a frontispiece: the
 * chapter heads are set in the demo face, and the image the chapter
 * opens on is centred with air around it.
 */
export const BOOK_LANDING_CSS = `@page {
  size: 5.5in 8.5in;
  margin: 0.7in 0.6in 0.8in 0.7in;

  @bottom-center {
    content: counter(page);
    font-size: 10pt;
  }
}

/* The wider margin is the spine on both sides of the spread. */
@page :left {
  margin-left: 0.6in;
  margin-right: 0.7in;

  @top-center {
    content: "The King of Elfland's Daughter";
    font-size: 10pt;
  }
}

@page :right {
  @top-center {
    content: string(chapter);
    font-size: 8pt;
  }
}

/* Don't show the page counter at the beginning of chapters */
@page :first {
  @bottom-center {
    content: ""
  }
}

/* A chapter's opening page carries no head. */
@page :first {
  @top-center { content: none; }
}

book {
  font-size: 12pt;
  line-height: 1.4;
  text-align: justify;
  hyphens: auto;
}

section {
  break-before: recto;
}

h2 {
  string-set: chapter content();
  font-family: 'im fell english sc';
  font-size: 12pt;
  font-weight: 400;
  text-align: center;
}

h3 {
  font-family: 'im fell english sc';
  font-size: 16pt;
  text-align: center;
  margin-bottom: 14pt;
}

/* The display face has no italic, and a head is the wrong place to
   ask a browser to slant one for it. */
h3 em {
  font-style: normal;
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

img {
  text-align: center;
  margin-top: .25in;
  margin-bottom: .25in;
}
h3 + p::first-letter {
  font-family: 'im fell english sc';
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
 * Every construct the mapping names, in one manuscript: the blocks
 * and inlines the content tree has a counterpart for, and the ones
 * it has none for, which are set as prose.
 */
export const MAPPING_MD = `## The Levant Papers

The letter reached Marsh on a Tuesday, in *the second post*, and it was
**not** what he had been waiting for. It began \`Dear sir\` and ended
without a name, which is [the whole of the difficulty](https://example.com).

> Nothing in the file said where the ship had gone.
>
> > And nothing said who had asked.

---

What the vocabulary has no room for is set as prose and reported:

1. a numbered list, one paragraph per item
2. and its second item

| construct | becomes |
|---|---|
| a table | one paragraph per cell |

\`\`\`
a code block, one paragraph
\`\`\`

Marsh was ~~certain~~ almost certain, and wrote to the shipping office
that afternoon.
`;

/**
 * Every demo the site mounts, and the poster each one is drawn on
 * top of until it does.
 *
 * `images` names the files a demo's manuscript refers to and
 * `fonts` the faces its stylesheet names beyond the bundled one,
 * both relative to `fixtures/`. The build reads them for the poster;
 * the island fetches the images from where the build puts them, and
 * the faces from where the site's own CSS serves them.
 *
 * A face carries the family and weight the engine reads out of its
 * name table, which is what a stylesheet selects it by and what the
 * site serves it as. The poster build holds the file to that.
 */
export const DEMOS = {
  /** The landing page's split view, and the docs' playground. */
  playground: () => ({
    name: 'the-king-of-elflands-daughter.md',
    markdown: opening('corpus/the-king-of-elflands-daughter.md', 1),
    css: BOOK_LANDING_CSS,
    images: ['images/lord.png'],
    fonts: [
      {
        url: 'fonts/IMFellEnglishSC-Regular.ttf',
        family: 'im fell english sc',
        weight: 400,
      },
    ],
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
    name: 'pride-and-prejudice.md',
    markdown: chapter('corpus/pride-and-prejudice.md', 1),
    css: fixture('styled.css'),
    // Nothing fetches a url for the demo, so the images the
    // manuscript names are handed over as urls it fetches itself.
    images: ['images/plate.jpg', 'images/fleuron.png'],
    page: 1,
  }),
  /** Every construct the mapping names, including the ones that warn. */
  mapping: () => ({
    name: 'chapter-01.md',
    markdown: MAPPING_MD,
    css: BOOK_CSS,
    dialect: 'gfm',
    page: 1,
  }),
  /** A stylesheet with declarations outside the subset, and a book anyway. */
  warnings: () => ({
    name: 'pride-and-prejudice.md',
    markdown: chapter('corpus/pride-and-prejudice.md', 1),
    css: OUTSIDE_CSS,
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
  return { images: [], fonts: [], ...build() };
}
