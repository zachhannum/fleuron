//! The subset description and the parser agree, and the docs page is
//! the description rendered.
//!
//! `FLEURON_UPDATE_DOCS=1 cargo test -p fleuron --test css_subset`
//! rewrites the generated regions of `docs/css-subset.mdx`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use fleuron::Warning;
use fleuron::style::subset::{Descriptor, Property, Selector, Subset};
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

/// As `warns_on_line_two`, pinned to the column `rule` writes a
/// declaration at.
fn warns_at_line_two(css: &str, naming: &str) {
    assert_eq!(
        warns_on_line_two(css, naming),
        3,
        "{css}\nwarned at the wrong column"
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
        // A margin box reads its own `content`, checked below.
        let shadowed = subset
            .page
            .margin_box_properties
            .iter()
            .any(|own| own.name == property.name);
        if !shadowed {
            accepts_property("@page { @top-center", property);
        }
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
    for name in &subset.selectors.first_line_properties {
        let property = subset
            .properties
            .iter()
            .find(|property| &property.name == name)
            .unwrap_or_else(|| panic!("`{name}` is not a property of the description"));
        accepts_property("p::first-line", property);
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
        ("background-image", "url(paper.png)"),
        ("border-radius", "3pt"),
        ("border-top-width", "1pt"),
        ("float", "left"),
        ("display", "block"),
        ("transform", "rotate(1deg)"),
        ("--ornament", "\"❦\""),
        ("counter-increment", "page"),
        ("font-variant", "small-caps"),
        ("font", "italic 11pt serif"),
        ("height", "10em"),
        ("border-spacing", "2pt"),
        ("inset", "0"),
        ("clip-path", "circle()"),
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

    for property in &subset.properties {
        if subset
            .selectors
            .first_line_properties
            .contains(&property.name)
        {
            continue;
        }
        let example = property.examples.first().expect("a value that parses");
        warns_at_line_two(
            &rule("p::first-line", &format!("{}: {example}", property.name)),
            &property.name,
        );
    }

    warns_at_line_two(&rule("p", "font-size: bigger"), "font-size");
    warns_at_line_two(&rule("p", "margin-top: 1vw"), "margin-top");
    warns_at_line_two(&rule("p", "color: transparent"), "color");
    warns_at_line_two(&rule("p", "shape-outside: circle(4em)"), "shape-outside");
    warns_at_line_two(
        &rule("p", "shape-outside: polygon(0 0, 100% 0)"),
        "shape-outside",
    );
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
        "::marker",
        "::selection",
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
/// construct sits. Returns the column.
fn warns_on_line_two(css: &str, naming: &str) -> u32 {
    let warnings = warnings(css);
    let hit = warnings
        .iter()
        .find(|warning| warning.message.contains(naming));
    let Some(hit) = hit else {
        panic!("{css}\ndid not warn naming `{naming}`: {warnings:?}");
    };
    hit.origin
        .as_deref()
        .and_then(|origin| origin.strip_prefix("sheet.css:2:"))
        .and_then(|column| column.parse::<u32>().ok())
        .unwrap_or_else(|| panic!("{css}\nwarned without the position: {hit:?}"))
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

/// Every stylesheet the docs page shows parses without a warning.
///
/// The generated regions are excluded: they are value-definition
/// syntax rather than CSS, and the tests above cover them. What is
/// left is the snippets written by hand, which is where a page drifts
/// from the engine.
#[test]
fn every_stylesheet_on_the_docs_page_parses() {
    let path = workspace_root().join("docs/css-subset.mdx");
    let page = std::fs::read_to_string(&path).expect("the docs page");
    let snippets = stylesheets(&page);
    assert!(!snippets.is_empty(), "the page shows no stylesheet");
    for snippet in snippets {
        let warnings = warnings(&snippet);
        assert!(warnings.is_empty(), "{snippet}\nwarned: {warnings:?}");
    }
}

/// The ```css blocks outside the generated regions.
fn stylesheets(page: &str) -> Vec<String> {
    let mut snippets = Vec::new();
    for prose in
        page.split("{/* generated: ")
            .map(|region| match region.find("{/* end generated */}") {
                Some(end) => &region[end..],
                None => region,
            })
    {
        let mut rest = prose;
        while let Some(open) = rest.find("```css\n") {
            let from = &rest[open + "```css\n".len()..];
            let close = from.find("```").expect("a closed code fence");
            snippets.push(from[..close].to_string());
            rest = &from[close + "```".len()..];
        }
    }
    snippets
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
    let selectors = &subset.selectors;
    let page = &subset.page;
    match name {
        "elements" => format!(
            "The element names come from markdown:\n\n{}",
            block(&selectors.elements)
        ),
        "compounds" => format!(
            "A compound selector is one of these, optionally followed by any of \
             the pseudo-classes below.\n\n{}\n\n{}",
            pairs("compound", true, &selectors.compounds),
            block(&names(&selectors.pseudo_classes))
        ),
        "combinators" => format!(
            "A combinator joins two compound selectors, and a `{}` separates the \
             selectors in a list (`{}`).\n\n{}",
            selectors.list.name,
            selectors.list.example,
            pairs("combinator", false, &selectors.combinators)
        ),
        "pseudo-elements" => format!(
            "The pseudo-elements are {}.",
            list(&names(&selectors.pseudo_elements))
        ),
        "first-line-properties" => format!(
            "`::first-line` accepts only these properties:\n\n{}",
            block(&selectors.first_line_properties)
        ),
        "declaration" => format!("A declaration is `{}`.", subset.declaration),
        "defaults" => format!("```css\n{}```", fleuron::style::USER_AGENT_CSS),
        "text-properties" => properties_table(subset, true),
        "block-properties" => properties_table(subset, false),
        "units" => format!(
            "A length can be written in any of these units, and the engine \
             converts them all to points.\n\n{}",
            block(&subset.units)
        ),
        "page-grammar" => page_grammar(subset),
        "page-sizes" => format!(
            "A named page size is one of these, portrait unless `landscape` \
             follows.\n\n{}",
            block(&names_of_sizes(&page.sizes))
        ),
        "counter-styles" => format!(
            "A counter style is one of these.\n\n{}",
            block(&page.counter_styles)
        ),
        "margin-boxes" => margin_boxes(subset),
        "font-face" => {
            let rows: Vec<(Vec<&str>, &str, String)> = subset
                .font_face
                .descriptors
                .iter()
                .map(|descriptor| {
                    (
                        vec![descriptor.name.as_str()],
                        descriptor.syntax.as_str(),
                        first_example(&descriptor.name, &descriptor.examples),
                    )
                })
                .collect();
            format!("{}\n\n{}", table("descriptor", &rows), syntax(&rows))
        }
        other => panic!("no generator for region `{other}`"),
    }
}

/// A flat vocabulary, set apart from the prose. A paragraph of inline
/// code spans is a wall; the same names on their own lines are read at
/// a glance.
fn block(names: &[String]) -> String {
    let mut out = String::from("```");
    let mut line = String::new();
    for name in names {
        if !line.is_empty() && line.len() + name.len() + 1 > 60 {
            out.push('\n');
            out.push_str(&line);
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(name);
    }
    out.push('\n');
    out.push_str(&line);
    out.push_str("\n```");
    out
}

/// Selector syntax against a selector that uses it. `code` is whether
/// the left column holds syntax, which is set in code, or the name of
/// a combinator, which is prose.
fn pairs(heading: &str, code: bool, selectors: &[Selector]) -> String {
    let mut out = format!("| {heading} | example |\n|---|---|");
    for selector in selectors {
        let name = if code {
            format!("`{}`", selector.name)
        } else {
            selector.name.clone()
        };
        out.push_str(&format!("\n| {name} | `{}` |", selector.example));
    }
    out
}

fn names(selectors: &[Selector]) -> Vec<String> {
    selectors
        .iter()
        .map(|selector| selector.name.clone())
        .collect()
}

fn names_of_sizes(sizes: &[fleuron::style::subset::PageSize]) -> Vec<String> {
    sizes.iter().map(|size| size.name.clone()).collect()
}

/// The properties that do or do not inherit: a table of what to write,
/// then the value syntax as a block. The syntax is too wide for a
/// column, and a cell it overflows scrolls the example out of sight.
fn properties_table(subset: &Subset, inherited: bool) -> String {
    let wanted: Vec<&Property> = subset
        .properties
        .iter()
        .filter(|property| property.inherited == inherited)
        .collect();
    let rows = merged(&wanted);
    format!("{}\n\n{}", table("property", &rows), syntax(&rows))
}

/// The value syntax, aligned into one block. A row that merged several
/// properties into one line of the table gets a line each here, which
/// is what a reader looking a property up expects to find.
fn syntax(rows: &[(Vec<&str>, &str, String)]) -> String {
    let width = rows
        .iter()
        .flat_map(|(names, _, _)| names)
        .map(|name| name.len() + 1)
        .max()
        .unwrap_or(0);
    let mut out = String::from("```css\n");
    for (names, syntax, _) in rows {
        for name in names {
            out.push_str(&format!("{:width$} {syntax};\n", format!("{name}:")));
        }
    }
    out.push_str("```");
    out
}

/// Consecutive properties with the same syntax share a row, and the
/// row's example is written for the first of them.
fn merged<'a>(properties: &[&'a Property]) -> Vec<(Vec<&'a str>, &'a str, String)> {
    let mut rows: Vec<(Vec<&str>, &str, String)> = Vec::new();
    for property in properties {
        match rows.last_mut() {
            Some((names, syntax, _)) if *syntax == property.syntax => names.push(&property.name),
            _ => rows.push((
                vec![&property.name],
                &property.syntax,
                first_example(&property.name, &property.examples),
            )),
        }
    }
    rows
}

/// One declaration a reader can copy, out of the values the
/// description says parse. A value that turns the property off shows
/// nothing about it, so it is taken only where it is all there is.
fn first_example(name: &str, examples: &[String]) -> String {
    let off = ["none", "normal", "auto"];
    let value = examples
        .iter()
        .find(|example| !off.contains(&example.as_str()))
        .or_else(|| examples.first())
        .unwrap_or_else(|| panic!("`{name}` describes no example value"));
    format!("{name}: {value}")
}

/// What to write, one row per property. The value syntax is not a
/// column: it is wider than the content the page is set in.
fn table(heading: &str, rows: &[(Vec<&str>, &str, String)]) -> String {
    let mut out = format!("| {heading} | example |\n|---|---|");
    for (names, _, example) in rows {
        let names: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
        out.push_str(&format!(
            "\n| {} | `{}` |",
            names.join(", "),
            example.replace('|', "\\|")
        ));
    }
    out
}

fn page_grammar(subset: &Subset) -> String {
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
    format!("```css\n{grammar}\n```")
}

fn margin_boxes(subset: &Subset) -> String {
    let (paints, silent): (Vec<_>, Vec<_>) = subset
        .page
        .margin_boxes
        .iter()
        .partition(|margin_box| margin_box.paints);
    let named = |boxes: &[&fleuron::style::subset::MarginBoxDescription]| {
        boxes
            .iter()
            .map(|margin_box| format!("@{}", margin_box.name))
            .collect::<Vec<_>>()
    };
    format!(
        "The engine draws these margin boxes:\n\n{}\n\nThese parse but draw \
         nothing:\n\n{}",
        block(&named(&paints)),
        block(&named(&silent))
    )
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
    "row-gap", "scale", "shape-margin", "shape-outside", "size", "src", "string-set", "tab-size", "table-layout",
    "text-align", "text-align-last", "text-combine-upright", "text-decoration",
    "text-decoration-color", "text-decoration-line", "text-decoration-style",
    "text-decoration-thickness", "text-emphasis", "text-indent", "text-justify",
    "text-orientation", "text-overflow", "text-rendering", "text-shadow", "text-transform",
    "text-underline-offset", "text-underline-position", "text-wrap", "top", "transform",
    "transform-origin", "transition", "translate", "unicode-bidi", "unicode-range",
    "vertical-align", "visibility", "white-space", "widows", "width", "will-change",
    "word-break", "word-spacing", "word-wrap", "writing-mode", "z-index",
];
