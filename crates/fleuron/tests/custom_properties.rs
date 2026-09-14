//! Custom properties: a value named once, on the root, a section or a
//! paragraph, and read with `var()` wherever a value is expected.

use fleuron::content::Book;
use fleuron::fonts::{FontRegistry, bundled_registry};
use fleuron::images::Assets;
use fleuron::layout::layout_book;
use fleuron::pages::DrawItem;
use fleuron::style::{
    Break, Color, ComputedStyle, Edges, MarginBox, Source, StyleTree, Stylesheets,
};
use fleuron::wire;
use fleuron_markdown::Options;

/// A chapter opening, a paragraph, and a paragraph inside a quotation.
const MANUSCRIPT: &str =
    "## The Quay\n\nThe wind came off the water.\n\n> The harbour lights went out.\n";

const GULLIVER: &str = include_str!("../../../fixtures/gulliver-excerpt.md");

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| bundled_registry().expect("bundled font parses"))
}

fn book(markdown: &str) -> Book {
    let (sections, warnings) =
        fleuron_markdown::to_sections(markdown, "book.md", &Options::default());
    assert!(warnings.is_empty(), "the frontend warned: {warnings:?}");
    fleuron_markdown::assemble(fleuron_markdown::frontmatter(markdown), sections)
}

fn compile(book: &Book, css: &str) -> StyleTree {
    Stylesheets::parse(&[Source::author("book.css", css)]).compile(book, registry())
}

/// The computed style of the first element with this name.
fn style_of<'a>(styles: &'a StyleTree, element: &str) -> &'a ComputedStyle {
    let node = styles
        .nodes()
        .iter()
        .find(|node| node.element == element)
        .unwrap_or_else(|| panic!("the book has no `{element}`"));
    &styles.styles()[node.style as usize]
}

/// Every warning, as its message and where it points.
fn warned(styles: &StyleTree) -> Vec<(&str, Option<&str>)> {
    styles
        .warnings()
        .iter()
        .map(|warning| (warning.message.as_str(), warning.origin.as_deref()))
        .collect()
}

/// Acceptance: `:root { --accent: #d6075e }` with
/// `h2 { color: var(--accent) }` sets the heading in that colour.
#[test]
fn the_heading_takes_the_colour_the_root_names() {
    let book = book(MANUSCRIPT);
    let styles = compile(
        &book,
        ":root { --accent: #d6075e }\nh2 { color: var(--accent) }",
    );
    assert_eq!(warned(&styles), []);
    let accent = Color::rgb(0xd6, 0x07, 0x5e);
    assert_eq!(style_of(&styles, "h2").color, accent);

    let output = layout_book(&book, &styles, registry(), &Assets::none());
    let heading: Vec<Color> = output
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            DrawItem::Text { text, color, .. } if text.starts_with("The Quay") => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(heading, [accent]);
}

/// Acceptance: a custom property set on `section` reaches a paragraph
/// inside it, and one set on the paragraph beats it.
#[test]
fn a_custom_property_on_the_paragraph_beats_the_one_on_the_section() {
    let book = book(MANUSCRIPT);
    let styles = compile(
        &book,
        "section { --tone: #444444 }\n\
         p { color: var(--tone) }\n\
         blockquote > p { --tone: #008000 }",
    );
    assert_eq!(warned(&styles), []);
    let colours: Vec<Color> = styles
        .nodes()
        .iter()
        .filter(|node| node.element == "p")
        .map(|node| styles.styles()[node.style as usize].color)
        .collect();
    assert_eq!(
        colours,
        [Color::rgb(0x44, 0x44, 0x44), Color::rgb(0, 0x80, 0)]
    );
}

/// Acceptance: `var(--missing, 12pt)` resolves to 12pt.
#[test]
fn a_name_that_holds_nothing_takes_its_fallback() {
    let book = book(MANUSCRIPT);
    let styles = compile(&book, "p { font-size: var(--missing, 12pt) }");
    assert_eq!(warned(&styles), []);
    assert_eq!(style_of(&styles, "p").font_size, 12.0);
}

/// Acceptance: `var(--missing)` on `font-size` leaves the inherited
/// size and warns with the line and column. It overrides the size
/// written before it in the same rule, and every paragraph it matches
/// shares the one warning.
#[test]
fn a_name_that_holds_nothing_leaves_the_inherited_size_and_warns() {
    let book = book(MANUSCRIPT);
    let css =
        "section { font-size: 14pt }\np {\n  font-size: 9pt;\n  font-size: var(--missing);\n}";
    let styles = compile(&book, css);
    assert_eq!(style_of(&styles, "p").font_size, 14.0);
    assert_eq!(
        warned(&styles),
        [(
            "Custom property `--missing` has no value. `font-size` takes its inherited value.",
            Some("book.css:4:3")
        )]
    );
}

/// Acceptance: a cycle warns and does not hang.
///
/// Part: the properties that read a name on the cycle take their
/// initial value, or their inherited value where they inherit, and
/// each warning names the line and column.
#[test]
fn a_cycle_warns_and_does_not_hang() {
    let book = book(MANUSCRIPT);
    let css = ":root {\n  --a: var(--b);\n  --b: var(--a);\n}\n\
               h2 {\n  color: rgb(1, 2, 3);\n  color: var(--a);\n  break-after: var(--b);\n}";
    let styles = compile(&book, css);
    let heading = style_of(&styles, "h2");
    assert_eq!(heading.color, Color::BLACK);
    assert_eq!(heading.break_after, Break::Auto);
    assert_eq!(
        warned(&styles),
        [
            (
                "Custom property `--b` depends on itself. `--b` has no value.",
                Some("book.css:3:3")
            ),
            (
                "Custom property `--a` depends on itself. `--a` has no value.",
                Some("book.css:2:3")
            ),
            (
                "Custom property `--a` has no value. `color` takes its inherited value.",
                Some("book.css:7:3")
            ),
            (
                "Custom property `--b` has no value. `break-after` takes its initial value.",
                Some("book.css:8:3")
            ),
        ]
    );
}

/// Part: substitution that produces a value the property cannot parse
/// warns as an unsupported value does.
#[test]
fn a_substituted_value_the_property_cannot_read_warns_as_unsupported() {
    let book = book(MANUSCRIPT);
    let styles = compile(
        &book,
        ":root { --size: bigger }\np {\n  font-size: var(--size);\n}",
    );
    assert_eq!(style_of(&styles, "p").font_size, 11.0);
    assert_eq!(
        warned(&styles),
        [(
            "Unsupported value for `font-size`. `font-size` takes its inherited value.",
            Some("book.css:3:3")
        )]
    );
}

/// Part: `var()` substitutes wherever a value is expected: as part of
/// a shorthand, in `@page`, and in a page margin box. The last two
/// read the custom properties of the book's root.
#[test]
fn var_substitutes_in_shorthands_pages_and_margin_boxes() {
    let book = book(MANUSCRIPT);
    let css = ":root {\n  --edge: 30pt;\n  --accent: #d6075e;\n}\n\
               blockquote {\n  margin: var(--edge) 0;\n  border-left: 2pt solid var(--accent);\n}\n\
               @page {\n  margin: var(--edge);\n  @bottom-center { content: counter(page); color: var(--accent) }\n}";
    let styles = compile(&book, css);
    assert_eq!(warned(&styles), []);
    let accent = Color::rgb(0xd6, 0x07, 0x5e);

    let quotation = style_of(&styles, "blockquote");
    assert_eq!((quotation.margin.top, quotation.margin.left), (30.0, 0.0));
    assert_eq!(quotation.border.left.color, Some(accent));

    let page = styles.default_page();
    assert_eq!(page.geometry.margin, Edges::all(30.0));
    let folio = page
        .margin_box(MarginBox::BottomCenter)
        .expect("the page has a folio");
    assert_eq!(folio.style.color, accent);
}

/// Acceptance: a book whose sheet uses no custom property produces a
/// display structure byte-identical to before the change. The
/// checked-in snapshots hold what it was. Here, a style tree with no
/// custom property describes itself as it did, and the fixture book
/// encodes to the same bytes whether its sheet writes each value out
/// or names it once.
#[test]
fn a_sheet_without_custom_properties_sets_the_book_unchanged() {
    let book = book(GULLIVER);
    let written = "@page { margin: 54pt 48pt }\n\
                   book { font-size: 10.5pt; text-align: justify }\n\
                   h1, h2 { color: #d6075e; margin-bottom: 12pt }";
    let named = ":root {\n  --accent: #d6075e;\n  --gap: 12pt;\n}\n\
                 @page { margin: 54pt 48pt }\n\
                 book { font-size: 10.5pt; text-align: justify }\n\
                 h1, h2 { color: var(--accent); margin-bottom: var(--gap) }";
    let encoded = |css: &str| {
        let styles = compile(&book, css);
        assert_eq!(warned(&styles), []);
        let output = layout_book(&book, &styles, registry(), &Assets::none());
        wire::encode(&output).expect("a display structure encodes")
    };
    assert_eq!(encoded(written), encoded(named));

    let described = serde_json::to_string(&compile(&book, written)).expect("a style tree");
    assert!(!described.contains("\"custom\""), "{described}");
}
