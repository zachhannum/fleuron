//! fleuron-core: paged-media layout for book-shaped documents.
//!
//! The pipeline is one-way:
//!
//! ```text
//! content tree + style tree ─► box tree ─► line layout ─► fragmentation ─► pages
//! ```

pub mod content;
pub mod layout;
pub mod pages;
pub mod style;

/// Non-fatal problems surfaced during style compilation, layout, or
/// fragmentation: unsupported CSS, missing fonts, low effective DPI…
#[derive(Debug, Clone, serde::Serialize)]
pub struct Warning {
    pub message: String,
    /// Source location when one exists (CSS line, content node id).
    pub origin: Option<String>,
}

/// Everything the engine produces for one run: pages plus diagnostics.
#[derive(Debug, serde::Serialize)]
pub struct LayoutOutput {
    pub pages: Vec<pages::Page>,
    pub warnings: Vec<Warning>,
}
