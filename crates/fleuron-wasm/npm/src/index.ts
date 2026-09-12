/**
 * fleuron in a worker: markdown and CSS in, a display structure or PDF
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
  styleOp,
  type Applied,
  type Failed,
  type Folios,
  type Metadata,
  type NodeSource,
  type Op,
  type Rendered,
  type Request,
  type Response,
  type Sheet,
  type Source,
  type Superseded,
  type Want,
} from './protocol.js';
export { Preview, type PreviewOptions } from './preview.js';
export { VERSION } from './version.js';
export { faceFamily, paintPage, type PaintOptions } from './svg.js';
export {
  PAGE_BACKGROUND,
  PAGE_FURNITURE,
  WIRE_VERSION,
  WireError,
  decodeDisplayList,
  wireVersionOf,
  type Asset,
  type BackgroundItem,
  type DrawItem,
  type AxisSetting,
  type FaceAttributes,
  type Features,
  type FontRefEntry,
  type Glyph,
  type ImageItem,
  type Intrinsic,
  type LayoutOutput,
  type Page,
  type RectItem,
  type Side,
  type SourceRange,
  type TextItem,
  type Warning,
} from './wire.js';
export { Session, render, renderPdf, wireVersion } from '../wasm/fleuron.js';
export { default as initWasm, initSync, type InitInput, type SyncInitInput } from '../wasm/fleuron.js';
