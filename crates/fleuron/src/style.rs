//! The style tree: resolved styling.
//!
//! Invariant: layout never reads settings. Three fixed origins —
//! built-in defaults, UI settings (serialized as a virtual stylesheet),
//! user CSS — cascade into one StyleTree, and every downstream pass
//! consumes only that.
