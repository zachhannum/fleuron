//! fleuron: paged-media layout for book-shaped documents.
//!
//! The pipeline is one-way:
//!
//! ```text
//! content tree + style tree ─► box tree ─► line layout ─► fragmentation ─► pages
//! ```

#![deny(missing_docs)]

pub mod content;
pub mod fonts;
pub mod images;
pub mod layout;
pub mod linebox;
pub mod lines;
pub mod pages;
pub mod pdf;
pub mod session;
pub mod style;
pub mod wire;

/// Non-fatal problems surfaced during style compilation, layout, or
/// fragmentation: unsupported CSS, missing fonts, low effective DPI…
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Warning {
    /// What went wrong, in one line.
    pub message: String,
    /// Source location when one exists (CSS line, content node id).
    pub origin: Option<String>,
}

/// Everything the engine produces for one run: pages plus diagnostics.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayoutOutput {
    /// The typeset pages, in reading order.
    pub pages: Vec<pages::Page>,
    /// The fonts this run used, indexed by `font_id`: both painters
    /// and the PDF writer resolve ids through this table.
    pub fonts: Vec<fonts::FontRefEntry>,
    /// The images this run placed, indexed by `DrawItem::Image.asset`.
    /// Layout sized them from their headers and decoded nothing; a
    /// painter takes the url back to its own pixels.
    pub assets: Vec<images::Asset>,
    /// Everything the run had to complain about.
    pub warnings: Vec<Warning>,
}
