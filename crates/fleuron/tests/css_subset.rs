//! The subset description and the parser agree, and the docs page is
//! the description rendered.
//!
//! `FLEURON_UPDATE_DOCS=1 cargo test -p fleuron --test css_subset`
//! rewrites the generated regions of `docs/css-subset.mdx`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fleuron::Warning;
use fleuron::style::subset::{Descriptor, Property, Subset};
use fleuron::style::{Source, Stylesheets};

fn warnings(css: &str) -> Vec<Warning> {
    Stylesheets::parse(&[Source::author("sheet.css", css)])
        .warnings()
        .to_vec()
}

fn parses(css: &str) {
    let warnings = warnings(css);
    assert!(warnings.is_empty(), "{css}\nwarned: {warnings:?}");
}

/// The declaration in a rule with the given prelude, written so a
/// warning lands at line 2, column 3.
fn rule(prelude: &str, declaration: &str) -> String {
    format!("{prelude} {{\n  {declaration};\n}}")
}

fn warns_at_line_two(css: &str, naming: &str) {
    let warnings = warnings(css);
    let hit = warnings
        .iter()
        .find(|warning| warning.message.contains(naming));
    let Some(hit) = hit else {
        panic!("{css}\ndid not warn naming `{naming}`: {warnings:?}");
    };
    assert_eq!(
        hit.origin.as_deref(),
        Some("sheet.css:2:3"),
        "{css}\nwarned without the position: {hit:?}"
    );
}

/// Every example parses, and every keyword parses alone or appears in
/// an example.
fn accepts(prelude: &str, name: &str, examples: &[String], keywords: &[String]) {
    for example in examples {
        parses(&rule(prelude, &format!("{name}: {example}")));
    }
    for keyword in keywords {
        let inside_example = examples.iter().any(|example| {
            example
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                .any(|word| word == keyword)
        });
        if inside_example {
            continue;
        }
        parses(&rule(prelude, &format!("{name}: {keyword}")));
    }
}

fn accepts_property(prelude: &str, property: &Property) {
    accepts(
        prelude,
        &property.name,
        &property.examples,
        &property.keywords,
    );
}

fn accepts_descriptor(prelude: &str, descriptor: &Descriptor) {
    accepts(
        prelude,
        &descriptor.name,
        &descriptor.examples,
        &descriptor.keywords,
    );
}

/// Every property in the description parses without a warning on a
/// value the description names, and so does everything else the
/// description lists: selectors, page selectors, margin boxes, sizes,
/// counter styles, units, colour names and descriptors.
#[test]
fn every_listed_property_parses_a_value_the_description_names() {
    let subset = Subset::describe();

    for property in &subset.properties {
        accepts_property("p", property);
        accepts_property("@page { @top-center", property);
    }
    for property in &subset.page.properties {
        accepts_property("@page", property);
    }
    for property in &subset.page.margin_box_properties {
        accepts_property("@page { @top-center", property);
    }
    for descriptor in &subset.font_face.descriptors {
        accepts_descriptor("@font-face", descriptor);
    }

    for element in &subset.selectors.elements {
        parses(&rule(element, "color: black"));
    }
    let listed = subset
        .selectors
        .compounds
        .iter()
        .chain(&subset.selectors.combinators)
        .chain(std::iter::once(&subset.selectors.list))
        .chain(&subset.selectors.pseudo_classes)
        .chain(&subset.selectors.pseudo_elements);
    for selector in listed {
        parses(&rule(&selector.example, "color: black"));
    }
    assert!(subset.declaration.contains("!important?"));
    parses(&rule("p", "color: black !important"));

    assert!(subset.page.prelude.starts_with("<name>?"));
    parses(&rule("@page chapter", "margin: 1in"));
    for selector in &subset.page.selectors {
        parses(&rule(&format!("@page :{selector}"), "margin: 1in"));
        parses(&rule(&format!("@page chapter:{selector}"), "margin: 1in"));
    }
    for margin_box in &subset.page.margin_boxes {
        parses(&rule(
            &format!("@page {{ @{}", margin_box.name),
            "content: counter(page)",
        ));
    }
    for size in &subset.page.sizes {
        parses(&rule("@page", &format!("size: {}", size.name)));
        parses(&rule("@page", &format!("size: {} landscape", size.name)));
    }
    for style in &subset.page.counter_styles {
        parses(&rule(
            "@page { @top-center",
            &format!("content: counter(page, {style})"),
        ));
    }
    for unit in &subset.units {
        parses(&rule("p", &format!("margin-top: 1{unit}")));
    }
    for color in &subset.color_names {
        parses(&rule("p", &format!("color: {color}")));
    }
}

/// A property outside the description warns, naming line and column.
/// So does a selector, a page selector or an at-rule outside it.
#[test]
fn a_property_outside_the_description_warns_naming_line_and_column() {
    let subset = Subset::describe();
    let described: BTreeSet<&str> = subset
        .properties
        .iter()
        .map(|property| property.name.as_str())
        .collect();

    let outside = [
        ("background", "white"),
        ("border", "1px solid black"),
        ("padding", "1em"),
        ("float", "left"),
        ("display", "block"),
        ("transform", "rotate(1deg)"),
        ("--ornament", "\"❦\""),
        ("counter-increment", "page"),
        ("font-variant", "small-caps"),
        ("font", "italic 11pt serif"),
        ("width", "10em"),
        ("position", "absolute"),
    ];
    for (name, value) in outside {
        assert!(!described.contains(name), "{name} is in the description");
        warns_at_line_two(&rule("p", &format!("{name}: {value}")), name);
        warns_at_line_two(
            &rule("@page { @top-center", &format!("{name}: {value}")),
            name,
        );
    }
    warns_at_line_two(&rule("@page", "font-size: 10pt"), "font-size");
    warns_at_line_two(&rule("@font-face", "size: a4"), "size");

    warns_at_line_two(&rule("p", "font-size: bigger"), "font-size");
    warns_at_line_two(&rule("p", "margin-top: 1vw"), "margin-top");
    warns_at_line_two(&rule("p", "color: transparent"), "color");
    warns_at_line_two(&rule("@page", "size: tabloid"), "size");
    warns_at_line_two(
        &rule("@page { @top-center", "content: counter(page, hebrew)"),
        "content",
    );

    for selector in [
        ":hover",
        ":link",
        ":any-link",
        ":visited",
        ":active",
        ":focus",
        ":lang(en)",
        ":dir(ltr)",
        "::first-line",
        "::before",
        "::after",
    ] {
        let css = format!("p {{ color: black }}\n  p{selector} {{ color: red }}");
        warns_on_line_two(&css, "selector");
    }
    warns_on_line_two(
        "p { color: black }\n  @page :verso { margin: 1in }",
        "@page",
    );
    warns_on_line_two(
        "p { color: black }\n  @media print { p { color: red } }",
        "@media",
    );
}

/// A warning naming `naming` on line 2, at whichever column the
/// construct sits.
fn warns_on_line_two(css: &str, naming: &str) {
    let warnings = warnings(css);
    let hit = warnings
        .iter()
        .find(|warning| warning.message.contains(naming));
    let Some(hit) = hit else {
        panic!("{css}\ndid not warn naming `{naming}`: {warnings:?}");
    };
    let column = hit
        .origin
        .as_deref()
        .and_then(|origin| origin.strip_prefix("sheet.css:2:"))
        .and_then(|column| column.parse::<u32>().ok());
    assert!(
        column.is_some(),
        "{css}\nwarned without the position: {hit:?}"
    );
}

/// A property the engine reads is in the description: every property
/// CSS defines that the parser accepts without a warning is one the
/// description lists, so a property added to the engine alone is
/// caught here rather than shipped undescribed.
#[test]
fn a_property_the_engine_reads_is_in_the_description() {
    let subset = Subset::describe();
    let described: BTreeSet<&str> = subset
        .properties
        .iter()
        .chain(&subset.page.properties)
        .chain(&subset.page.margin_box_properties)
        .map(|property| property.name.as_str())
        .chain(
            subset
                .font_face
                .descriptors
                .iter()
                .map(|descriptor| descriptor.name.as_str()),
        )
        .collect();

    let mut undescribed = Vec::new();
    for name in CSS_PROPERTIES {
        if described.contains(name) {
            continue;
        }
        for value in [
            "inherit", "initial", "none", "auto", "normal", "0", "1em", "black",
        ] {
            let preludes = ["p", "@page", "@page { @top-center", "@font-face"];
            for prelude in preludes {
                let css = rule(prelude, &format!("{name}: {value}"));
                if !warnings(&css)
                    .iter()
                    .any(|warning| warning.message.contains(name))
                {
                    undescribed.push(format!("{prelude} {{ {name}: {value} }}"));
                }
            }
        }
    }
    assert!(
        undescribed.is_empty(),
        "read but not described: {undescribed:#?}"
    );
}

/// The generated regions of the docs page are what the description
/// renders. Set `FLEURON_UPDATE_DOCS=1` to rewrite them.
#[test]
fn the_docs_page_is_the_description_rendered() {
    let path = workspace_root().join("docs/css-subset.mdx");
    let page = std::fs::read_to_string(&path).expect("the docs page");
    let rendered = render(&page, &Subset::describe());
    if std::env::var_os("FLEURON_UPDATE_DOCS").is_some() {
        std::fs::write(&path, &rendered).expect("write the docs page");
        return;
    }
    assert_eq!(
        rendered, page,
        "docs/css-subset.mdx is stale; FLEURON_UPDATE_DOCS=1 cargo test -p fleuron --test css_subset"
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// The page with every generated region replaced by what the
/// description says. A region is bracketed by `{/* generated: name */}`
/// and `{/* end generated */}`, and the prose between the regions is
/// left alone.
fn render(page: &str, subset: &Subset) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(start) = rest.find("{/* generated: ") {
        let (before, from_marker) = rest.split_at(start);
        out.push_str(before);
        let marker_end = from_marker.find(" */}").expect("a closed marker");
        let name = &from_marker["{/* generated: ".len()..marker_end];
        let after_marker = &from_marker[marker_end + " */}".len()..];
        let end = after_marker
            .find("{/* end generated */}")
            .expect("an end marker");
        out.push_str(&format!("{{/* generated: {name} */}}\n\n"));
        out.push_str(&region(name, subset));
        out.push_str("\n\n{/* end generated */}");
        rest = &after_marker[end + "{/* end generated */}".len()..];
    }
    out.push_str(rest);
    out
}

fn region(name: &str, subset: &Subset) -> String {
    match name {
        "selectors" => selectors(subset),
        "properties" => properties(subset),
        "units" => format!(
            "Lengths are {}, and everything computes to points.",
            list(&subset.units)
        ),
        "page" => page(subset),
        "font-face" => {
            let rows = subset
                .font_face
                .descriptors
                .iter()
                .map(|descriptor| (vec![descriptor.name.as_str()], descriptor.syntax.as_str()))
                .collect();
            table("descriptor", rows)
        }
        other => panic!("no generator for region `{other}`"),
    }
}

fn selectors(subset: &Subset) -> String {
    let selectors = &subset.selectors;
    let compounds: Vec<String> = selectors
        .compounds
        .iter()
        .map(|compound| format!("`{}` (`{}`)", compound.name, compound.example))
        .collect();
    let combinators: Vec<String> = selectors
        .combinators
        .iter()
        .map(|combinator| format!("{} (`{}`)", combinator.name, combinator.example))
        .collect();
    let pseudo_classes: Vec<String> = selectors
        .pseudo_classes
        .iter()
        .map(|pseudo| pseudo.name.clone())
        .collect();
    let pseudo_elements: Vec<String> = selectors
        .pseudo_elements
        .iter()
        .map(|pseudo| pseudo.name.clone())
        .collect();
    [
        format!(
            "Element names are the markdown vocabulary: {}.",
            list(&selectors.elements)
        ),
        format!(
            "A compound is {}, with any of the pseudo-classes {}.",
            either(&compounds),
            list(&pseudo_classes)
        ),
        format!(
            "Compounds join by the {} combinators, and selectors list with `{}` (`{}`).",
            joined(&combinators),
            selectors.list.name,
            selectors.list.example
        ),
        format!("The pseudo-element {}.", list(&pseudo_elements)),
    ]
    .join("\n\n")
}

fn properties(subset: &Subset) -> String {
    let (inherited, not_inherited): (Vec<&Property>, Vec<&Property>) = subset
        .properties
        .iter()
        .partition(|property| property.inherited);
    format!(
        "A declaration is `{}`.\n\nText, inherited:\n\n{}\n\nBlock box, not inherited:\n\n{}",
        subset.declaration,
        table("property", merged(&inherited)),
        table("property", merged(&not_inherited))
    )
}

/// Consecutive properties with the same syntax share a row.
fn merged<'a>(properties: &[&'a Property]) -> Vec<(Vec<&'a str>, &'a str)> {
    let mut rows: Vec<(Vec<&str>, &str)> = Vec::new();
    for property in properties {
        match rows.last_mut() {
            Some((names, syntax)) if *syntax == property.syntax => names.push(&property.name),
            _ => rows.push((vec![&property.name], &property.syntax)),
        }
    }
    rows
}

fn table(heading: &str, rows: Vec<(Vec<&str>, &str)>) -> String {
    let mut out = format!("| {heading} | values |\n|---|---|");
    for (names, syntax) in rows {
        let names: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
        out.push_str(&format!(
            "\n| {} | `{}` |",
            names.join(", "),
            syntax.replace('|', "\\|")
        ));
    }
    out
}

fn page(subset: &Subset) -> String {
    let page = &subset.page;
    let mut grammar = format!("@page {} {{\n", page.prelude);
    for property in &page.properties {
        grammar.push_str(&format!("  {}: {};\n", property.name, property.syntax));
    }
    grammar.push_str("  @<margin-box> {\n");
    for property in &page.margin_box_properties {
        grammar.push_str(&format!("    {}: {};\n", property.name, property.syntax));
    }
    grammar.push_str("    /* text properties */\n  }\n}");

    let sizes: Vec<String> = page.sizes.iter().map(|size| size.name.clone()).collect();
    let (paints, silent): (Vec<_>, Vec<_>) = page
        .margin_boxes
        .iter()
        .partition(|margin_box| margin_box.paints);
    let named = |boxes: &[&fleuron::style::subset::MarginBoxDescription]| {
        boxes
            .iter()
            .map(|margin_box| format!("@{}", margin_box.name))
            .collect::<Vec<_>>()
    };
    [
        format!("```css\n{grammar}\n```"),
        format!(
            "`<page-size>` is {}, portrait unless `landscape` follows.",
            list(&sizes)
        ),
        format!("`<counter-style>` is {}.", list(&page.counter_styles)),
        format!(
            "The margin boxes that paint are {}. {} parse and paint nothing.",
            list(&named(&paints)),
            list(&named(&silent))
        ),
    ]
    .join("\n\n")
}

/// Names in backticks, comma separated, `and` before the last.
fn list(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("`{item}`")).collect();
    joined(&quoted)
}

/// Comma separated, `and` before the last.
fn joined(items: &[String]) -> String {
    conjoined(items, "and")
}

/// Comma separated, `or` before the last.
fn either(items: &[String]) -> String {
    conjoined(items, "or")
}

fn conjoined(items: &[String], conjunction: &str) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} {conjunction} {}", rest.join(", "), last),
    }
}

/// The properties CSS defines, as a host might send any of them.
#[rustfmt::skip]
const CSS_PROPERTIES: &[&str] = &[
    "align-content", "align-items", "align-self", "all", "animation", "aspect-ratio",
    "backdrop-filter", "background", "background-color", "background-image", "background-position",
    "background-repeat", "background-size", "block-size", "border", "border-bottom",
    "border-collapse", "border-color", "border-image", "border-left", "border-radius",
    "border-right", "border-spacing", "border-style", "border-top", "border-width", "bottom",
    "box-decoration-break", "box-shadow", "box-sizing", "break-after", "break-before",
    "break-inside", "caption-side", "clear", "clip", "clip-path", "color", "column-count",
    "column-gap", "column-rule", "column-span", "column-width", "columns", "contain", "content",
    "counter-increment", "counter-reset", "counter-set", "cursor", "direction", "display",
    "empty-cells", "filter", "flex", "flex-basis", "flex-direction", "flex-flow", "flex-grow",
    "flex-shrink", "flex-wrap", "float", "font", "font-family", "font-feature-settings",
    "font-kerning", "font-language-override", "font-optical-sizing", "font-palette", "font-size",
    "font-size-adjust", "font-stretch", "font-style", "font-synthesis", "font-variant",
    "font-variant-alternates", "font-variant-caps", "font-variant-east-asian",
    "font-variant-ligatures", "font-variant-numeric", "font-variant-position",
    "font-variation-settings", "font-weight", "gap", "grid", "grid-area", "grid-column",
    "grid-row", "grid-template", "grid-template-areas", "grid-template-columns",
    "grid-template-rows", "hanging-punctuation", "height", "hyphenate-character",
    "hyphenate-limit-chars", "hyphens", "image-rendering", "initial-letter",
    "initial-letter-align", "inline-size", "inset", "isolation", "justify-content",
    "justify-items", "justify-self", "left", "letter-spacing", "line-break", "line-clamp",
    "line-height", "list-style", "list-style-image", "list-style-position", "list-style-type",
    "margin", "margin-block", "margin-bottom", "margin-inline", "margin-left", "margin-right",
    "margin-top", "marks", "mask", "max-block-size", "max-height", "max-inline-size", "max-width",
    "min-block-size", "min-height", "min-inline-size", "min-width", "mix-blend-mode",
    "object-fit", "object-position", "opacity", "order", "orphans", "outline", "overflow",
    "overflow-wrap", "padding", "padding-block", "padding-bottom", "padding-inline",
    "padding-left", "padding-right", "padding-top", "page", "page-break-after",
    "page-break-before", "page-break-inside", "perspective", "place-content", "place-items",
    "place-self", "pointer-events", "position", "quotes", "resize", "right", "rotate",
    "row-gap", "scale", "shape-outside", "size", "src", "string-set", "tab-size", "table-layout",
    "text-align", "text-align-last", "text-combine-upright", "text-decoration",
    "text-decoration-color", "text-decoration-line", "text-decoration-style",
    "text-decoration-thickness", "text-emphasis", "text-indent", "text-justify",
    "text-orientation", "text-overflow", "text-rendering", "text-shadow", "text-transform",
    "text-underline-offset", "text-underline-position", "text-wrap", "top", "transform",
    "transform-origin", "transition", "translate", "unicode-bidi", "unicode-range",
    "vertical-align", "visibility", "white-space", "widows", "width", "will-change",
    "word-break", "word-spacing", "word-wrap", "writing-mode", "z-index",
];
