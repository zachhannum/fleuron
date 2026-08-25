//! Snapshot of the `LayoutOutput` font table: the wire-format shape
//! of `FontRefEntry`.

use fleuron::LayoutOutput;
use fleuron::fonts::{FontRefEntry, GenericFamily, bundled_registry};

#[test]
fn layout_output_font_table_snapshot() {
    let registry = bundled_registry().expect("bundled font parses");
    let fonts: Vec<FontRefEntry> = (0..registry.len() as u16)
        .filter_map(|id| registry.font_ref(id).cloned())
        .collect();
    let output = LayoutOutput {
        pages: vec![],
        fonts,
        warnings: vec![],
    };
    let generics: Vec<_> = [
        GenericFamily::Serif,
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
    ]
    .iter()
    .map(|g| (g.keyword(), registry.generic(*g)))
    .collect();
    insta::assert_json_snapshot!((output, generics));
}
