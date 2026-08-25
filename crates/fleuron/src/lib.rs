//! fleuron: paged-media layout for book-shaped documents.
//!
//! The pipeline is one-way:
//!
//! ```text
//! content tree + style tree ─► box tree ─► line layout ─► fragmentation ─► pages
//! ```

pub mod content;
pub mod fonts;
pub mod layout;
pub mod linebox;
pub mod lines;
pub mod pages;
pub mod pdf;
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
    /// The fonts this run used, indexed by `font_id`: both painters
    /// and the PDF writer resolve ids through this table.
    pub fonts: Vec<fonts::FontRefEntry>,
    pub warnings: Vec<Warning>,
}
