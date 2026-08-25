//! fleuron-wasm: layout in a worker, bytes out.
//!
//! Contract with the host (Orca): the engine never touches the DOM.
//! Inputs cross as serialized bytes; outputs come back as one
//! transferable ArrayBuffer — the postcard-encoded display list, or PDF
//! bytes on the export path. See the Orca Integration milestone.
