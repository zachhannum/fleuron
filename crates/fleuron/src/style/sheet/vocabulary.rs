//! The tables the subset document is written from: what a selector
//! may hold, and what a declaration looks like.

/// The pseudo-classes a selector may use, as they are written, with
/// one use of each that parses. The selector parser is the
/// `selectors` crate's, so this table is checked against it rather
/// than read by it.
pub(crate) const PSEUDO_CLASSES: &[(&str, &str)] = &[
    (":first-child", "p:first-child"),
    (":last-child", "p:last-child"),
    (":only-child", "p:only-child"),
    (":nth-child()", "p:nth-child(2n+1)"),
    (":nth-last-child()", "p:nth-last-child(2)"),
    (":first-of-type", "p:first-of-type"),
    (":last-of-type", "p:last-of-type"),
    (":only-of-type", "p:only-of-type"),
    (":nth-of-type()", "p:nth-of-type(2)"),
    (":nth-last-of-type()", "p:nth-last-of-type(2)"),
    (":empty", "p:empty"),
    (":root", ":root"),
    (":is()", ":is(h1, h2)"),
    (":where()", ":where(h1, h2)"),
    (":not()", "p:not(:first-child)"),
    (":has()", "p:has(em)"),
];

/// What a compound selector is made of besides pseudo-classes, with
/// one use of each that parses.
pub(crate) const COMPOUNDS: &[(&str, &str)] = &[
    ("<element>", "p"),
    ("*", "section > *"),
    (".<class>", "p.epigraph"),
    ("#<id>", "#frontispiece"),
];

/// How selectors join in a list, with one use that parses.
pub(crate) const SELECTOR_LIST: (&str, &str) = (",", "h1, h2");

/// The shape of one declaration.
pub(crate) const DECLARATION: &str = "<property>: <value> !important?";

/// The shape of one custom property declaration.
pub(crate) const CUSTOM_PROPERTY: &str = "--<name>: <value> !important?";

/// The function that puts a custom property's value in a declaration.
pub(crate) const VAR: &str = "var( --<name> [, <fallback> ]? )";

/// The combinators between two compounds, by their CSS names, with
/// one use of each that parses.
pub(crate) const COMBINATORS: &[(&str, &str)] = &[
    ("descendant", "section p"),
    ("child", "section > p"),
    ("next-sibling", "h1 + p"),
    ("subsequent-sibling", "h1 ~ p"),
];

/// The pseudo-elements, as they are written, with one use of each
/// that parses.
pub(crate) const PSEUDO_ELEMENTS: &[(&str, &str)] = &[
    ("::first-letter", "p::first-letter"),
    ("::first-line", "p::first-line"),
    ("::before", "a::before"),
    ("::after", "a::after"),
];

/// What `::first-line` takes. `font-size`, `font-variant-caps`,
/// `letter-spacing` and `text-transform` change the width of the
/// shaped run, which is what the second breaking pass is for.
/// `color` and the rules `text-decoration` draws are paint alone,
/// and break the same either way.
pub(crate) const FIRST_LINE_PROPERTIES: &[&str] = &[
    "color",
    "font-size",
    "font-variant-caps",
    "letter-spacing",
    "text-transform",
    "text-decoration-line",
    "text-decoration-color",
    "text-decoration-style",
    "text-decoration-thickness",
    "text-decoration",
];
