/**
 * The browser run: the example harness, driven.
 *
 * The headless harness beside this one proves the numbers: the SVG
 * puts every glyph at the x the display structure gave it. What it cannot
 * prove is that those numbers reach a screen, so this opens the
 * harness in a real browser, pages the fixture book through it, and
 * checks one page against a raster of the PDF the same run exported.
 *
 * The two rasters come from different engines, Chromium's and
 * poppler's, so they are compared as ink rather than as pixels: both
 * are reduced to a grid of coverage cells, and a glyph in the wrong
 * place moves ink between cells where antialiasing cannot.
 */

import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { chromium } from 'playwright';

/**
 * How far apart the two rasters may be, in ink per cell out of 255,
 * and on average over the page.
 *
 * Where the numbers come from: the painter as it stands scores 14 and
 * 1.4 against poppler, and a painter that puts every baseline one
 * point out scores 28 and 4.8. The bar sits between them, so the
 * check fails on an error a point wide and passes the pixel the two
 * rasterisers disagree about on their own.
 */
const CELL = 24;
/** And on average, over the page. */
const MEAN = 3;
/**
 * Where the ink's centre of mass may differ across the page, in
 * pixels. It comes out at 0.004 against poppler on one machine and
 * 0.27 on another, and a baseline a point out of place would move it
 * by two.
 */
const CENTRE = 1;
/** The side of one coverage cell, in device pixels. */
const GRID = 16;
/** Points to pixels for both sides of the comparison. */
const ZOOM = 2;
/**
 * The page the two rasters are compared on, which is one of running
 * text.
 *
 * The two sides resample an image differently, so a page whose ink is
 * mostly one tonal block measures the resampling rather than where
 * the glyphs are.
 */
const COMPARED = 3;
/**
 * And the page the box model is compared on: the excerpt's inventory
 * of the man-mountain's pockets, which `fixtures/styled.css` sets in
 * a bordered, padded, tinted box carried over a page turn.
 *
 * A background is the one thing a painter lays under the text rather
 * than over it, so a painter that got the order wrong differs from
 * the export here.
 */
const BOXED = 22;

let failures = 0;

function check(what: string, passed: boolean, detail = ''): void {
  console.log(`  ${passed ? 'ok  ' : 'FAIL'}  ${what}${detail === '' ? '' : `\n          ${detail}`}`);
  if (!passed) {
    failures += 1;
  }
}

interface Server {
  address(): { port: number };
  close(callback: () => void): void;
}

const { serve } = (await import(
  new URL('../../../../examples/preview/serve.mjs', import.meta.url).href
)) as { serve(port?: number): Promise<Server> };

console.log('fleuron browser run: the example harness, driven\n');

const server = await serve(0);
const { port } = server.address();
const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });
const broke: string[] = [];
page.on('pageerror', (error) => broke.push(String(error)));

await page.goto(`http://127.0.0.1:${port}/examples/preview/`, { waitUntil: 'load' });
await page.waitForSelector('body[data-ready="yes"]', { timeout: 120_000 });

// Most of what follows turns to a page and reads its markup back:
// installed once here rather than duplicated at every call site.
await page.evaluate(() => {
  globalThis.__settledOnPage = async (folio, limit = 200) => {
    for (
      let waited = 0;
      document.querySelector('#preview svg')?.getAttribute('data-page') !== String(folio) &&
      waited < limit;
      waited += 1
    ) {
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  };
});

const pages = await page.evaluate(() => globalThis.preview.pages as number);
console.log(`  the harness sets the fixture book in ${pages} pages\n`);
check('the fixture book reaches the browser', pages > 0);

// Virtualization: a page turn to a neighbour the last turn already
// prefetched paints synchronously, with nothing awaited in between; a
// jump past the held window does not paint at once, and only lands
// once the worker answers.
const roundTrips = await page.evaluate(async () => {
  const preview = globalThis.preview;
  const showing = (): string | null => document.querySelector('#preview svg')?.getAttribute('data-page') ?? null;
  // A page away from the mount's own opening cascade of renders
  // (each image and the stylesheet is its own render, and every one
  // fires its own prefetch), settled and given a further beat for
  // its own neighbour prefetch to land before anything is measured.
  preview.page = 10;
  await globalThis.__settledOnPage(10);
  for (let waited = 0; waited < 200; waited += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  preview.page = 11;
  const immediatelyOnNeighbour = showing();
  preview.page = 25;
  const immediatelyOnJump = showing();
  await globalThis.__settledOnPage(25);
  return { immediatelyOnNeighbour, immediatelyOnJump, landed: showing() };
});
check(
  'a page turn to an already-prefetched neighbour paints at once',
  roundTrips.immediatelyOnNeighbour === '11',
  `showed page ${roundTrips.immediatelyOnNeighbour} right after the turn`,
);
check(
  'jumping past the held window does not paint until the worker answers',
  roundTrips.immediatelyOnJump !== '25',
  `showed page ${roundTrips.immediatelyOnJump} right after the jump`,
);
check(
  'and it paints the page asked for once the answer arrives',
  roundTrips.landed === '25',
  `landed on page ${roundTrips.landed}`,
);

// A host that reads the page back off onRender and re-assigns it on
// every one of its own re-renders (a React effect keyed on the
// output it just received, say) must not see that turn into a render
// of its own: reassigning the page already on screen is a no-op.
const idempotent = await page.evaluate(async () => {
  const preview = globalThis.preview;
  preview.page = 5;
  await globalThis.__settledOnPage(5);
  const folio = document.getElementById('folio');
  let mutations = 0;
  const observer = new MutationObserver(() => {
    mutations += 1;
  });
  observer.observe(folio as Node, { childList: true, characterData: true, subtree: true });
  for (let i = 0; i < 5; i += 1) {
    preview.page = preview.page;
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  observer.disconnect();
  return mutations;
});
check(
  'reassigning the page already on screen does not itself trigger another render',
  idempotent === 0,
  `${idempotent} mutation(s) observed`,
);

// The same host pattern, but for a jump still in flight rather than
// one already landed: repeating an identical assignment while it is
// pending joins the request already on its way instead of opening a
// second one for the same page.
const deduped = await page.evaluate(async () => {
  const preview = globalThis.preview;
  preview.page = 1;
  await globalThis.__settledOnPage(1);
  let sent = 0;
  const original = Worker.prototype.postMessage;
  Worker.prototype.postMessage = function counted(this: Worker, ...args: unknown[]) {
    sent += 1;
    return (original as (...rest: unknown[]) => void).apply(this, args);
  } as typeof Worker.prototype.postMessage;
  try {
    preview.page = 20;
    const afterFirst = sent;
    preview.page = 20;
    preview.page = 20;
    const afterRepeats = sent;
    await globalThis.__settledOnPage(20);
    return {
      afterFirst,
      afterRepeats,
      landed: document.querySelector('#preview svg')?.getAttribute('data-page'),
    };
  } finally {
    Worker.prototype.postMessage = original;
  }
});
check(
  'reassigning an in-flight jump target opens no request beyond the one already sent for it',
  deduped.afterRepeats === deduped.afterFirst,
  `${deduped.afterFirst} message(s) for the jump, ${deduped.afterRepeats} after two repeats`,
);
check('and the jump it joined still lands', deduped.landed === '20');

// A target left and come back to before its own request landed is
// still in flight, not merely the most recently asked-for one: this
// is the same join as an immediate repeat, just by a different route.
const awayAndBack = await page.evaluate(async () => {
  const preview = globalThis.preview;
  preview.page = 1;
  await globalThis.__settledOnPage(1);
  let sent = 0;
  const original = Worker.prototype.postMessage;
  Worker.prototype.postMessage = function counted(this: Worker, ...args: unknown[]) {
    sent += 1;
    return (original as (...rest: unknown[]) => void).apply(this, args);
  } as typeof Worker.prototype.postMessage;
  try {
    preview.page = 30;
    preview.page = 31;
    const away = sent;
    preview.page = 30;
    const back = sent;
    await globalThis.__settledOnPage(30);
    return { away, back, landed: document.querySelector('#preview svg')?.getAttribute('data-page') };
  } finally {
    Worker.prototype.postMessage = original;
  }
});
check(
  'a target left and returned to before it landed opens no request beyond what leaving it already sent',
  awayAndBack.back === awayAndBack.away,
  `${awayAndBack.away} message(s) leaving page 30, ${awayAndBack.back} coming back to it`,
);
check('and it still lands', awayAndBack.landed === '30');

// Every page, painted and on screen: the page is turned to each in
// turn and the element that lands is the one the display structure asked
// for, with text on it. Most of these pages are outside the window
// the preview holds, so turning to one asks the worker for it; this
// waits for that reply rather than reading the element the turn
// before it left behind.
const painted = await page.evaluate(async (count: number) => {
  const preview = globalThis.preview;
  const wrong: string[] = [];
  for (let number = 1; number <= count; number += 1) {
    preview.page = number;
    await globalThis.__settledOnPage(number);
    const svg = document.querySelector('#preview svg');
    const runs = svg?.querySelectorAll('text').length ?? 0;
    if (svg?.getAttribute('data-page') !== String(number) || runs === 0) {
      wrong.push(`page ${number}: ${runs} runs, element says ${svg?.getAttribute('data-page')}`);
    }
  }
  preview.page = 1;
  return wrong;
}, pages);
check('every page paints, in the browser, from the bytes the worker sent', painted.length === 0, painted.slice(0, 3).join('; '));

// The face on screen is the file the engine shaped with, loaded from
// the module rather than fetched from anywhere.
const loaded = await page.evaluate(async () => {
  await document.fonts.ready;
  return document.fonts.check('12pt "fleuron-face-0"');
});
check('the face the engine shaped with is the face the browser draws', loaded);

// A face that never arrives falls through the painter's stack. The
// text is set in the wrong font, which is the point: it is set.
const fallback = await page.evaluate(() => {
  const svg = document.querySelector('#preview svg') as SVGSVGElement;
  const run = svg.querySelector('text') as SVGTextElement;
  const copy = run.cloneNode(true) as SVGTextElement;
  copy.setAttribute('font-family', '"fleuron-face-404", serif');
  svg.append(copy);
  const width = copy.getComputedTextLength();
  copy.remove();
  return width;
});
check('a missing face falls back visibly rather than painting nothing', fallback > 0, `${fallback.toFixed(1)}pt of text`);

// The export of the same run, rastered by something that is not a
// browser, compared with a screenshot of the page on screen. Both are
// made at the same zoom, so a pixel is a pixel on either side.
// The images the harness handed over are on the page, drawn from the
// same files the engine sized them by.
const drawn = await page.evaluate(async (count: number) => {
  const preview = globalThis.preview;
  let drawn = 0;
  for (let number = 1; number <= count; number += 1) {
    preview.page = number;
    await globalThis.__settledOnPage(number);
    drawn += document.querySelectorAll('#preview svg image').length;
  }
  preview.page = 1;
  return drawn;
}, pages);
check('the images the host handed over are painted, not outlined', drawn === 2, `${drawn} drawn`);

// Asset cache: leaving an image's page far enough behind revokes the
// blob url it painted from, and returning to that page paints it
// again from one freshly made rather than leaving it outlined for
// good.
const assetCache = await page.evaluate(async (count: number) => {
  const preview = globalThis.preview;
  let imagePage = -1;
  for (let number = 1; number <= count && imagePage === -1; number += 1) {
    preview.page = number;
    await globalThis.__settledOnPage(number);
    if (document.querySelectorAll('#preview svg image').length > 0) {
      imagePage = number;
    }
  }
  if (imagePage === -1) {
    return { imagePage };
  }
  let created = 0;
  let revoked = 0;
  const originalCreate = URL.createObjectURL;
  const originalRevoke = URL.revokeObjectURL;
  URL.createObjectURL = function counted(...args: unknown[]) {
    created += 1;
    return (originalCreate as (...rest: unknown[]) => string).apply(URL, args);
  } as typeof URL.createObjectURL;
  URL.revokeObjectURL = function counted(...args: unknown[]) {
    revoked += 1;
    return (originalRevoke as (...rest: unknown[]) => void).apply(URL, args);
  } as typeof URL.revokeObjectURL;
  try {
    const away = Math.min(imagePage + 20, count);
    preview.page = away;
    await globalThis.__settledOnPage(away);
    const revokedAfterLeaving = revoked;
    preview.page = imagePage;
    await globalThis.__settledOnPage(imagePage);
    return {
      imagePage,
      revokedAfterLeaving,
      createdAfterReturn: created,
      drawnAgain: document.querySelectorAll('#preview svg image').length,
    };
  } finally {
    URL.createObjectURL = originalCreate;
    URL.revokeObjectURL = originalRevoke;
  }
}, pages);
check(
  'an image no held page draws any more has its blob url revoked',
  assetCache.imagePage !== -1 && (assetCache.revokedAfterLeaving ?? 0) > 0,
  JSON.stringify(assetCache),
);
check(
  'and returning to its page paints it again from one freshly made',
  (assetCache.createdAfterReturn ?? 0) > 0 && (assetCache.drawnAgain ?? 0) > 0,
  JSON.stringify(assetCache),
);

/**
 * One page compared with the export: the preview photographed in the
 * browser, and the same page of the PDF the same run wrote rastered
 * by poppler. Both at the same zoom, so a pixel is a pixel on either
 * side.
 *
 * This is the check the two painters cannot both pass while drawing
 * different books. One draws glyph ids; the other draws characters
 * and lets the browser choose glyphs for them. So a property that
 * changes which glyphs a run is shaped from is a property the two can
 * disagree over, and every one of them belongs in a book this runs
 * over.
 */
async function comparedWithTheExport(what: string, compared: number): Promise<void> {
  await page.evaluate(async ([zoom, number]: [number, number]) => {
    globalThis.preview.zoom = zoom;
    globalThis.preview.page = number;
    // A page this far from the one the harness opened on is not one
    // the preview already held: wait for the worker's reply rather
    // than photograph the page the turn before this one left up.
    await globalThis.__settledOnPage(number);
    // The harness's own chrome puts the sheet at a fractional pixel,
    // and a screenshot of a fractional box is every glyph blurred
    // half a pixel sideways. The page under it is untouched.
    for (const part of document.querySelectorAll('header, footer, #export')) {
      part.remove();
    }
    document.body.setAttribute('style', 'margin: 0');
    document.querySelector('main')?.setAttribute('style', 'padding: 0');
    document.querySelector('.sheet')?.setAttribute('style', 'line-height: 0');
    scrollTo(0, 0);
  }, [ZOOM, compared] as [number, number]);
  const images = await page.evaluate(
    () => document.querySelectorAll('#preview svg image').length,
  );
  const pdf = await page.evaluate(async () => {
    const bytes = await globalThis.preview.exportPdf();
    return bytes === null ? null : [...bytes];
  });
  if (pdf === null) {
    throw new Error('nothing overtook the export, and it still came back superseded');
  }
  const work = mkdtempSync(join(tmpdir(), 'fleuron-browser-'));
  writeFileSync(join(work, 'book.pdf'), Buffer.from(pdf));

  const shot = await (await page.$('#preview svg'))?.screenshot({ type: 'png' });
  if (shot === undefined) {
    throw new Error('there was no page on screen to photograph');
  }
  const rastered = spawnSync(
    'pdftoppm',
    [
      '-f',
      String(compared),
      '-l',
      String(compared),
      '-singlefile',
      '-r',
      String(72 * ZOOM),
      '-png',
      join(work, 'book.pdf'),
      join(work, 'page'),
    ],
    { encoding: 'buffer' },
  );
  if (rastered.status !== 0) {
    console.log('  skip  the preview and the export raster the same (pdftoppm not installed)');
    if (process.env['FLEURON_WASM_REQUIRE_TOOLS'] === '1') {
      failures += 1;
    }
  } else {
    check(
      `${what}: the page the two rasters are compared on has no image`,
      images === 0,
      `${images} on page ${compared}`,
    );
    const reference = readFileSync(join(work, 'page.png'));
    const difference = await page.evaluate(
      async ([left, right, grid]: [string, string, number]) => {
        const load = async (source: string): Promise<ImageBitmap> =>
          createImageBitmap(await (await fetch(source)).blob());
        const [a, b] = await Promise.all([load(left), load(right)]);
        const width = Math.min(a.width, b.width);
        const height = Math.min(a.height, b.height);
        const ink = (image: ImageBitmap): Float64Array => {
          const canvas = new OffscreenCanvas(width, height);
          const context = canvas.getContext('2d') as OffscreenCanvasRenderingContext2D;
          context.fillStyle = '#ffffff';
          context.fillRect(0, 0, width, height);
          context.drawImage(image, 0, 0);
          const { data } = context.getImageData(0, 0, width, height);
          const out = new Float64Array(width * height);
          for (let at = 0; at < out.length; at += 1) {
            out[at] =
              255 -
              ((data[at * 4] ?? 0) * 0.299 +
                (data[at * 4 + 1] ?? 0) * 0.587 +
                (data[at * 4 + 2] ?? 0) * 0.114);
          }
          return out;
        };
        /** Ink gathered into a grid of cells, and how much of it there is. */
        const cells = (page: Float64Array): { of: Float64Array; total: number } => {
          const across = Math.ceil(width / grid);
          const of = new Float64Array(across * Math.ceil(height / grid));
          let total = 0;
          for (let y = 0; y < height; y += 1) {
            for (let x = 0; x < width; x += 1) {
              const value = page[y * width + x] ?? 0;
              const cell = Math.floor(y / grid) * across + Math.floor(x / grid);
              of[cell] = (of[cell] ?? 0) + value;
              total += value;
            }
          }
          return { of, total };
        };
        /** Where the ink sits, side to side. */
        const centre = (page: Float64Array): number => {
          let weighted = 0;
          let total = 0;
          for (let y = 0; y < height; y += 1) {
            for (let x = 0; x < width; x += 1) {
              const value = page[y * width + x] ?? 0;
              weighted += value * x;
              total += value;
            }
          }
          return weighted / total;
        };
        const [one, two] = [ink(a), ink(b)];
        const [first, second] = [cells(one), cells(two)];
        // One rasteriser lays a heavier stem than the other, and that
        // is not a disagreement about layout. Total ink is scaled out
        // before the cells are compared, so what is left is where the
        // ink is rather than how much of it there is.
        const heavier = first.total / second.total;
        let worst = 0;
        let sum = 0;
        for (let cell = 0; cell < first.of.length; cell += 1) {
          const apart = Math.abs((first.of[cell] ?? 0) - (second.of[cell] ?? 0) * heavier) / grid ** 2;
          worst = Math.max(worst, apart);
          sum += apart;
        }
        return {
          worst,
          mean: sum / first.of.length,
          centre: centre(one) - centre(two),
          heavier,
          width,
          height,
        };
      },
      [
        `data:image/png;base64,${shot.toString('base64')}`,
        `data:image/png;base64,${reference.toString('base64')}`,
        GRID,
      ] as [string, string, number],
    );
    const measured =
      `${difference.width}×${difference.height}px, worst cell ${difference.worst.toFixed(1)}/${CELL},` +
      ` mean ${difference.mean.toFixed(2)}/${MEAN}, centre ${difference.centre.toFixed(3)}px/${CENTRE}` +
      `, ${((difference.heavier - 1) * 100).toFixed(0)}% heavier on screen`;
    check(
      `${what}: the preview and the export put the same ink in the same places`,
      difference.worst <= CELL && difference.mean <= MEAN,
      measured,
    );
    check(
      `${what}: and put it at the same x, which is the one the display structure gave`,
      Math.abs(difference.centre) <= CENTRE,
      measured,
    );
  }
}

await comparedWithTheExport('running text', COMPARED);
await comparedWithTheExport('a bordered and tinted quotation', BOXED);

// The display-typography book, whose every page is set in the three
// properties that change which glyphs a run is shaped from. The
// opening page sets a title transformed to capitals and tracked,
// a chapter title and the chapter's opening line in the face's own
// small capitals, and tracked prose; the page after it sets a
// small-capital running head.
// A painter that drew the characters the manuscript spells, or asked
// the face for none of its features, parts company with the export
// here.
const typography = await page.evaluate(async () => {
  const [markdown, css] = await Promise.all([
    (await fetch('/fixtures/display-typography.md')).text(),
    (await fetch('/fixtures/display-typography.css')).text(),
  ]);
  await globalThis.preview.setStyle(css);
  await globalThis.preview.setMarkdown(markdown, 'display-typography.md');
  return {
    pages: globalThis.preview.pages as number,
    warnings: globalThis.preview.warnings.map((warning) => warning.message),
  };
});
check(
  'the display-typography sheet is in the subset the engine honours',
  typography.warnings.length === 0,
  typography.warnings.slice(0, 3).join('; '),
);
check('the display-typography book runs past its opening page', typography.pages >= 2);
await comparedWithTheExport('a transformed and tracked title', 1);
await comparedWithTheExport('a small-capital running head', 2);

// Selection overlay: a drag over the invisible layer selects it, not
// the glyphs underneath, so what copy yields is the manuscript's own
// casing rather than what a `text-transform` drew — and a selection
// spanning two lines joins them in reading order, with nothing
// duplicated or dropped.
const selection = await page.evaluate(async () => {
  globalThis.preview.page = 1;
  await globalThis.__settledOnPage(1);
  const doc = document;
  const frame = doc.querySelector('[data-fleuron="preview"]');
  const lines = [...doc.querySelectorAll('#preview svg text[data-selection-line]')] as SVGTextElement[];

  const copy = (range: Range): string | null => {
    const sel = doc.getSelection();
    sel?.removeAllRanges();
    sel?.addRange(range);
    const data = new DataTransfer();
    const event = new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true });
    frame?.dispatchEvent(event);
    sel?.removeAllRanges();
    return event.defaultPrevented ? data.getData('text/plain') : null;
  };

  const title = lines.find((line) => line.textContent === 'A Voyage to Lilliput');
  let titleCopy: string | null = null;
  if (title !== undefined) {
    const range = doc.createRange();
    range.selectNodeContents(title);
    titleCopy = copy(range);
  }

  const firstIndex = lines.findIndex((line) => (line.textContent ?? '').includes('Nottinghamshire'));
  let crossLine: { copied: string | null; expected: string } | null = null;
  if (firstIndex !== -1 && firstIndex + 1 < lines.length) {
    const first = lines[firstIndex] as SVGTextElement;
    const second = lines[firstIndex + 1] as SVGTextElement;
    const firstText = first.firstChild;
    const secondText = second.firstChild;
    if (firstText !== null && secondText !== null) {
      const firstContent = firstText.textContent ?? '';
      const secondContent = secondText.textContent ?? '';
      const cut = Math.min(10, secondContent.length);
      const range = doc.createRange();
      range.setStart(firstText, 3);
      range.setEnd(secondText, cut);
      crossLine = {
        copied: copy(range),
        expected: `${firstContent.slice(3)}\n${secondContent.slice(0, cut)}`,
      };
    }
  }

  return { lineCount: lines.length, titleCopy, crossLine };
});
check(
  "selecting the transformed title's own overlay copies the manuscript's own casing",
  selection.titleCopy === 'A Voyage to Lilliput',
  `${selection.lineCount} selection line(s); copied ${JSON.stringify(selection.titleCopy)}`,
);
check(
  'a selection spanning two lines joins them in reading order with nothing duplicated or dropped',
  selection.crossLine !== null && selection.crossLine.copied === selection.crossLine.expected,
  JSON.stringify(selection.crossLine),
);

// The list form, driven: the same sheet as the second of two
// layers, over a preset it overrides and a declaration the engine
// does not honour. What the layers set is what the one string set,
// to the byte, and the complaint names the layer it was written in.
const layered = await page.evaluate(async () => {
  const before = await globalThis.preview.exportPdf();
  const css = await (await fetch('/fixtures/display-typography.css')).text();
  await globalThis.preview.setStyle([
    {
      name: 'preset.css',
      css: 'book { font-size: 9pt }\np { text-rendering: geometricPrecision }\n',
    },
    { name: 'display-typography.css', css },
  ]);
  const after = await globalThis.preview.exportPdf();
  return {
    pages: globalThis.preview.pages,
    warnings: globalThis.preview.warnings.map(
      (warning) => `${warning.origin}: ${warning.message}`,
    ),
    same:
      before !== null &&
      after !== null &&
      before.length === after.length &&
      before.every((byte, at) => byte === after[at]),
  };
});
check(
  'a sheet sent as layers sets the book the same sheet set as one string',
  layered.pages === typography.pages && layered.same,
  `${layered.pages} pages against ${typography.pages}`,
);
check(
  'and a warning in a layer names that layer',
  layered.warnings.length === 1 && (layered.warnings[0] ?? '').startsWith('preset.css:2:'),
  layered.warnings.join('; '),
);

// An edit that shrinks the book past the page on screen: the fetch
// for the page that was showing comes back empty once the book no
// longer has it, and a second fetch lands the preview on a page the
// shorter book actually has, rather than a blank frame.
const shrunk = await page.evaluate(async () => {
  const preview = globalThis.preview;
  const showing = () => document.querySelector('#preview svg')?.getAttribute('data-page') ?? null;
  preview.page = preview.pages;
  await globalThis.__settledOnPage(preview.pages);
  const before = preview.pages;
  await preview.setMarkdown('# Short\n\nOne short paragraph is all there is now.\n', 'shrunk.md');
  return { before, after: preview.pages, page: preview.page, landed: showing() };
});
check(
  'an edit that shrinks the book past the page on screen still lands on a real page',
  shrunk.after < shrunk.before &&
    shrunk.page >= 1 &&
    shrunk.page <= shrunk.after &&
    shrunk.landed === String(shrunk.page),
  `${shrunk.before} pages showing page ${shrunk.before}, then ${shrunk.after} pages, landed on ${shrunk.landed}`,
);

check('nothing threw on the page', broke.length === 0, broke.slice(0, 2).join('; '));

await browser.close();
await new Promise<void>((resolve) => server.close(() => resolve()));

console.log(failures === 0 ? '\nall checks passed' : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);

declare global {
  var preview: {
    pages: number;
    page: number;
    zoom: number;
    warnings: { message: string; origin: string | null }[];
    exportPdf(): Promise<Uint8Array | null>;
    setStyle(css: string | { name: string; css: string }[]): Promise<void>;
    setMarkdown(text: string, name?: string): Promise<void>;
  };
  /** Waits until `#preview svg`'s `data-page` reads `folio`, or gives up after `limit` ticks. */
  var __settledOnPage: (folio: number, limit?: number) => Promise<void>;
}
