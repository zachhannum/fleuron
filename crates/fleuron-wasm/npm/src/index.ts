/**
 * fleuron in a worker: markdown and CSS in, a display list or PDF
 * bytes out.
 *
 * The host keeps a {@link Client}, the worker keeps an
 * {@link Engine}, and between them the engine's session keeps every
 * stage of the pipeline so that a second render pays for the edit
 * rather than for the book.
 *
 * {@link Preview} is all of that behind one object: an element, a
 * manuscript, and a page on screen.
 */

export { Client, SUPERSEDED, type Transport } from './client.js';
export { Engine, createEngine, type EngineOptions, type Reply } from './engine.js';
export {
  isFailed,
  isRendered,
  type Applied,
  type Failed,
  type Op,
  type Rendered,
  type Request,
  type Response,
  type Superseded,
  type Want,
} from './protocol.js';
export { Preview, type PreviewOptions } from './preview.js';
export { faceFamily, paintPage, type PaintOptions } from './svg.js';
export {
  WIRE_VERSION,
  WireError,
  decodeDisplayList,
  wireVersionOf,
  type DrawItem,
  type AxisSetting,
  type FaceAttributes,
  type FontRefEntry,
  type Glyph,
  type ImageItem,
  type LayoutOutput,
  type Page,
  type RectItem,
  type Side,
  type TextItem,
  type Warning,
} from './wire.js';
export { Session, render, renderPdf, wireVersion } from '../wasm/fleuron.js';
export { default as initWasm, initSync, type InitInput, type SyncInitInput } from '../wasm/fleuron.js';
