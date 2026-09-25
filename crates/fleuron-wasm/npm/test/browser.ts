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
/**
 * And the page the inline box is compared on: the excerpt's
 * cross-reference, which `fixtures/styled.css` puts on a tinted chip
 * with rounded corners.
 *
 * The reference runs over a line break, so the chip is two boxes, and
 * a painter that squared the wrong corners differs from the export
 * here.
 */
const CHIPPED = 28;

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


/**
 * The centre of one line of a link on screen, and points beside it, in
 * the viewport's pixels: `mark` is the mark's order among the marks
 * of the link at index `link`.
 */
async function markOnScreen(
  link: number,
  mark = 0,
): Promise<{ x: number; y: number; left: number; right: number; top: number; bottom: number } | null> {
  const handle = (await page.$$(`#preview svg rect[data-link="${link}"]`))[mark];
  if (handle === undefined) {
    return null;
  }
  await handle.scrollIntoViewIfNeeded();
  const box = await handle.boundingBox();
  return box === null
    ? null
    : {
        x: box.x + box.width / 2,
        y: box.y + box.height / 2,
        left: box.x,
        right: box.x + box.width,
        top: box.y,
        bottom: box.y + box.height,
      };
}

/** The page on screen after a click at a point, once any turn it asked for lands. */
async function clickAt(x: number, y: number): Promise<number> {
  await page.evaluate(() => {
    globalThis.followed.length = 0;
  });
  await page.mouse.click(x, y);
  for (let waited = 0; waited < 200; waited += 1) {
    const landed = await page.evaluate(
      () =>
        document.querySelector('#preview svg')?.getAttribute('data-page') ===
        String(globalThis.preview.page),
    );
    if (landed) {
      break;
    }
  }
  return page.evaluate(() => globalThis.preview.page);
}

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

// Links. A click on a cross-reference turns the preview to the page
// its target opens on, and `onLink` hears of it first. Under this
// sheet the target of the fixture book's cross-reference opens on the
// page of the link, and the contents book further down turns to
// another page.
const crossReference = await page.evaluate(async (count: number) => {
  const preview = globalThis.preview;
  for (let number = 1; number <= count; number += 1) {
    preview.page = number;
    await globalThis.__settledOnPage(number);
    if (document.querySelector('#preview svg rect[data-link]') !== null) {
      const line = [...document.querySelectorAll('#preview svg text[data-selection-line]')].find(
        (text) => (text.textContent ?? '').includes('chapter III'),
      );
      return { number, line: line?.textContent ?? null };
    }
  }
  return null;
}, pages);
check('the fixture book has a link on a page', crossReference !== null);
if (crossReference !== null) {
  const on = await markOnScreen(0);
  const landed = on === null ? 0 : await clickAt(on.x, on.y);
  const followed = await page.evaluate(() => globalThis.followed.map((link) => link.to));
  const to = followed[0];
  check(
    'a click on the cross-reference in the fixture book turns the preview to the page of its target',
    to?.kind === 'place' && landed === to.place.page + 1,
    `on page ${crossReference.number}, followed ${JSON.stringify(followed)}, landed on ${landed}`,
  );

  // The line the link is set on still selects and copies as it did:
  // a drag across the link selects the words under it and turns no
  // page, and copy yields the line.
  await page.evaluate(async (number: number) => {
    globalThis.preview.page = number;
    await globalThis.__settledOnPage(number);
  }, crossReference.number);
  const mark = await markOnScreen(0);
  const line = await page.evaluate(() => {
    const text = [...document.querySelectorAll('#preview svg text[data-selection-line]')].find(
      (element) => (element.textContent ?? '').includes('chapter III'),
    ) as SVGTextElement | undefined;
    const box = text?.getBoundingClientRect();
    return box === undefined ? null : { left: box.left, right: box.right };
  });
  let copied: string | null = null;
  let stayed = false;
  if (mark !== null && line !== null) {
    await page.evaluate(() => {
      globalThis.followed.length = 0;
    });
    await page.mouse.move(line.left + 1, mark.y);
    await page.mouse.down();
    await page.mouse.move(line.right - 1, mark.y, { steps: 8 });
    await page.mouse.up();
    copied = await page.evaluate(() => {
      const frame = document.querySelector('[data-fleuron="preview"]');
      const data = new DataTransfer();
      const event = new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true });
      frame?.dispatchEvent(event);
      return event.defaultPrevented ? data.getData('text/plain') : null;
    });
    stayed = await page.evaluate(
      (number: number) => globalThis.preview.page === number && globalThis.followed.length === 0,
      crossReference.number,
    );
    await page.evaluate(() => document.getSelection()?.removeAllRanges());
  }
  check(
    'a drag across a line with a link selects and copies it, and follows nothing',
    stayed && copied !== null && copied.includes('chapter III') && crossReference.line !== null &&
      crossReference.line.includes(copied.trim()),
    `copied ${JSON.stringify(copied)} from ${JSON.stringify(crossReference.line)}`,
  );
}

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
await comparedWithTheExport('a chip on a cross-reference', CHIPPED);

// The display-typography book, whose every page is set in the
// properties that change which glyphs a run is shaped from. The
// opening page sets a title transformed to capitals, tracked and in
// the face's first stylistic set, a chapter title and the chapter's
// opening line in the face's own small capitals, and tracked prose
// in old-style figures; the page after it sets a small-capital
// running head.
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

// The highlight follows the set line. Every character of a selection
// line is measured as the browser highlights it: each box meets the
// one before it, and all of them are as tall as the glyphs of the
// line's largest run, whatever its face and size.
const highlight = await page.evaluate(async () => {
  const faults: string[] = [];
  let lines = 0;
  const sizes = new Set<string>();
  for (const number of [1, 2]) {
    globalThis.preview.page = number;
    await globalThis.__settledOnPage(number);
    await document.fonts.ready;
    const svg = document.querySelector('#preview svg') as SVGSVGElement;
    const painted = [...svg.querySelectorAll('text:not([data-selection-line])')] as SVGTextElement[];
    for (const line of svg.querySelectorAll('text[data-selection-line]') as NodeListOf<SVGTextElement>) {
      const node = line.firstChild;
      const text = line.textContent ?? '';
      if (line.childNodes.length !== 1 || node === null) {
        faults.push(`page ${number}: a line of ${line.childNodes.length} nodes`);
        continue;
      }
      const y = line.getAttribute('y');
      const size = line.getAttribute('font-size') ?? '';
      const glyphs = painted.find(
        (run) => run.getAttribute('y') === y && run.getAttribute('font-size') === size,
      );
      if (glyphs === undefined) {
        faults.push(`page ${number}: no run at ${size}pt on the baseline ${y}`);
        continue;
      }
      const set = glyphs.getBoundingClientRect();
      const range = document.createRange();
      let previous: DOMRect | null = null;
      for (let at = 0; at < text.length; at += 1) {
        range.setStart(node, at);
        range.setEnd(node, at + 1);
        const box = range.getBoundingClientRect();
        if (Math.abs(box.top - set.top) > 0.5 || Math.abs(box.height - set.height) > 0.5) {
          faults.push(
            `page ${number} ${JSON.stringify(text.slice(0, 20))} character ${at} is ${box.height.toFixed(2)}px tall at ${box.top.toFixed(2)}, the set line ${set.height.toFixed(2)}px at ${set.top.toFixed(2)}`,
          );
          break;
        }
        if (previous !== null && Math.abs(box.left - previous.right) > 0.5) {
          faults.push(
            `page ${number} ${JSON.stringify(text.slice(0, 20))} character ${at} starts at ${box.left.toFixed(2)}, the one before ends at ${previous.right.toFixed(2)}`,
          );
          break;
        }
        previous = box;
      }
      lines += 1;
      sizes.add(size);
    }
  }
  return { faults, lines, sizes: [...sizes] };
});
check(
  'the highlight on a selected line is one band from its first character to its last',
  highlight.lines > 0 && !highlight.faults.some((fault) => fault.includes('starts at')),
  highlight.faults.filter((fault) => fault.includes('starts at')).slice(0, 3).join('; '),
);
check(
  'the highlight on each line is as tall as the set line, for every face and size on the page',
  highlight.sizes.length >= 3 && highlight.faults.length === 0,
  `sizes ${highlight.sizes.join(', ')}; ${highlight.faults.slice(0, 3).join('; ')}`,
);

// A drag that starts off the glyphs still selects: in the gap between
// two lines of a paragraph, from the line nearer to where it starts,
// and in the margin beside a line.
const between = await page.evaluate(async () => {
  globalThis.preview.page = 1;
  await globalThis.__settledOnPage(1);
  const lines = [...document.querySelectorAll('#preview svg text[data-selection-line]')] as SVGTextElement[];
  const at = lines.findIndex((line) => (line.textContent ?? '').includes('Nottinghamshire'));
  const first = lines[at]?.getBoundingClientRect();
  const second = lines[at + 1]?.getBoundingClientRect();
  return first === undefined || second === undefined
    ? null
    : {
        left: first.left,
        right: Math.min(first.right, second.right),
        above: first.bottom + (second.top - first.bottom) / 4,
        below: second.top - (second.top - first.bottom) / 4,
        opened: first.bottom < second.top,
        baseline: first.top + first.height / 2,
        next: lines[at + 1]?.textContent ?? '',
        line: lines[at]?.textContent ?? '',
      };
});
async function dragged(fromX: number, fromY: number, toX: number, toY: number): Promise<string> {
  await page.evaluate(() => document.getSelection()?.removeAllRanges());
  await page.mouse.move(fromX, fromY);
  await page.mouse.down();
  await page.mouse.move(toX, toY, { steps: 8 });
  await page.mouse.up();
  const selected = await page.evaluate(() => {
    const selection = document.getSelection();
    const inLayer = (node: Node | null | undefined): boolean =>
      node?.parentElement?.closest('[data-selection-layer]') !== null &&
      node?.parentElement?.closest('[data-selection-layer]') !== undefined;
    return selection !== null && inLayer(selection.anchorNode) && inLayer(selection.focusNode)
      ? selection.toString()
      : '';
  });
  await page.evaluate(() => document.getSelection()?.removeAllRanges());
  return selected;
}
if (between === null) {
  check('the display-typography book has two lines of a paragraph on its opening page', false);
} else {
  const fromAbove = await dragged(between.left + 30, between.above, between.right - 20, between.above);
  const fromBelow = await dragged(between.left + 30, between.below, between.right - 20, between.below);
  const fromMargin = await dragged(between.left - 12, between.baseline, between.right - 20, between.baseline);
  check(
    'a drag that starts between two lines selects text',
    between.opened &&
      fromAbove.length > 0 &&
      between.line.includes(fromAbove) &&
      fromBelow.length > 0 &&
      between.next.includes(fromBelow),
    `${JSON.stringify(fromAbove)} and ${JSON.stringify(fromBelow)}`,
  );
  check(
    "a drag that starts inside a line's box, off its glyphs, selects that line",
    fromMargin.length > 0 && between.line.includes(fromMargin.trim()) && between.line.startsWith(fromMargin.slice(0, 5)),
    JSON.stringify(fromMargin),
  );
}

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

// Links in a book written for them: a contents entry that prints only
// its page number, a link broken across two lines, and a link out of
// the book.
const contents = await page.evaluate(async () => {
  const prose =
    'It was the custom of the island that a stranger be kept at the gate until the ' +
    "emperor's council had heard of him, and the council sat but twice a month.\n\n";
  await globalThis.preview.setStyle('a::after { content: target-counter(attr(href url), page) }');
  await globalThis.preview.setMarkdown(
    '# Contents\n\nThe Hunter [](#the-hunter)\n\nIt was the custom of the island that a stranger be ' +
      'kept at the gate, and so the reader turns to [the long and winding account of the voyage to ' +
      'the island of the giants and the hunter](#the-hunter) before any other part.\n\n' +
      prose.repeat(30) +
      '# The Hunter\n\nThe hunt began at dawn.\n',
    'links.md',
  );
  globalThis.preview.page = 1;
  await globalThis.__settledOnPage(1);
  return {
    pages: globalThis.preview.pages,
    marks: [...document.querySelectorAll('#preview svg rect[data-link]')].map((mark) =>
      mark.getAttribute('data-link'),
    ),
  };
});
const target = contents.pages;
const printed = await markOnScreen(0);
check(
  'a contents entry with no text of its own is marked on its printed page number',
  printed !== null && contents.marks.length === 3,
  JSON.stringify(contents),
);
if (printed !== null) {
  const landed = await clickAt(printed.x, printed.y);
  check('and a click on the number turns to the page it prints', landed === target, `landed on ${landed} of ${target}`);
}
await page.evaluate(async () => {
  globalThis.preview.page = 1;
  await globalThis.__settledOnPage(1);
});
const first = await markOnScreen(1, 0);
const second = await markOnScreen(1, 1);
if (first === null || second === null) {
  check('a link broken across two lines is marked on both', false);
} else {
  const turns: number[] = [];
  for (const [x, y] of [
    [first.x, first.y],
    [second.x, second.y],
  ] as [number, number][]) {
    turns.push(await clickAt(x, y));
    await page.evaluate(async () => {
      globalThis.preview.page = 1;
      await globalThis.__settledOnPage(1);
    });
  }
  check(
    'a link broken across two lines is followed from both lines',
    turns.every((landed) => landed === target),
    turns.join(', '),
  );
  const misses: number[] = [];
  const between: [number, number][] = [
    [first.left - 6, first.y],
    [second.right + 6, second.y],
  ];
  if (second.top - first.bottom > 1) {
    between.push([first.x, (first.bottom + second.top) / 2]);
  }
  for (const [x, y] of between) {
    misses.push(await clickAt(x, y));
  }
  check(
    'and from nowhere between them',
    misses.every((landed) => landed === 1),
    `${between.length} clicks landed on ${misses.join(', ')}`,
  );
}

const outward = await page.evaluate(async () => {
  await globalThis.preview.setStyle('');
  await globalThis.preview.setMarkdown(
    '# Elsewhere\n\nSee [the society](https://example.com/society) for the rest.\n',
    'links.md',
  );
  globalThis.preview.page = 1;
  await globalThis.__settledOnPage(1);
  return globalThis.preview.pages;
});
const society = await markOnScreen(0);
const stayedOn = society === null ? 0 : await clickAt(society.x, society.y);
const heard = await page.evaluate(() => globalThis.followed.map((link) => link.to));
check(
  'an external link calls onLink with its url and turns no page',
  outward >= 1 &&
    stayedOn === 1 &&
    heard.length === 1 &&
    heard[0]?.kind === 'uri' &&
    heard[0].url === 'https://example.com/society' &&
    (await page.context().pages()).length === 1,
  JSON.stringify(heard),
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
  /** The links the harness's `onLink` heard of, most recent last. */
  var followed: { to: { kind: 'place'; node: number; place: { page: number } } | { kind: 'uri'; url: string } }[];
  /** Waits until `#preview svg`'s `data-page` reads `folio`, or gives up after `limit` ticks. */
  var __settledOnPage: (folio: number, limit?: number) => Promise<void>;
}
