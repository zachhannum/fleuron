//! `text-decoration`: the rules a sheet draws across a run, and
//! where the face puts them.
//!
//! One fixture, a paragraph with a link in it, set under sheets that
//! draw a rule under the link, colour it, thicken it and double it.

use fleuron::content::{Attributes, Block, Book, Inline, NodeId, Section};
use fleuron::fonts::{FontRegistry, FontSource, bundled_registry};
use fleuron::layout::Paginator;
use fleuron::pages::{DrawItem, Page};
use fleuron::style::{Color, Source, StyleTree, Stylesheets};

/// A face of a second family, so a rule can be read off two sets of
/// metrics. IM Fell English SC is 2048 units to the em and puts its
/// underline further from the baseline than the bundled family does.
const SECOND_FACE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/fonts/IMFellEnglishSC-Regular.ttf"
);

fn registry() -> &'static FontRegistry {
    static REGISTRY: std::sync::OnceLock<FontRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = bundled_registry().expect("bundled font parses");
        let bytes = std::fs::read(SECOND_FACE).expect("the second face is checked in");
        registry
            .add(FontSource::from_bytes(bytes).expect("the second face parses"))
            .expect("the second face registers");
        registry
    })
}

/// A page wide enough for the fixture to set on one line.
const PAGE_CSS: &str = "@page { size: 300pt 200pt; margin: 20pt }";

fn text(value: &str) -> Inline {
    Inline::Text {
        id: NodeId::UNASSIGNED,
        value: value.into(),
        attributes: Attributes::default(),
        position: None,
        span: None,
    }
}

fn book_of(inlines: Vec<Inline>) -> Book {
    let mut book = Book {
        metadata: Default::default(),
        sections: vec![Section {
            attributes: Default::default(),
            id: NodeId::UNASSIGNED,
            source: None,
            title: None,
            blocks: vec![Block::Paragraph {
                id: NodeId::UNASSIGNED,
                inlines,
                attributes: Attributes::default(),
                position: None,
                span: None,
            }],
            position: None,
            span: None,
        }],
    };
    book.assign_node_ids();
    book
}

/// The fixture: prose with a link in the middle of it.
fn linked(words: &str) -> Book {
    book_of(vec![
        text("He read "),
        Inline::Link {
            id: NodeId::UNASSIGNED,
            url: "https://example.com".into(),
            children: vec![text(words)],
            attributes: Attributes::default(),
            position: None,
            span: None,
        },
        text(" and closed the book."),
    ])
}

fn styles(book: &Book, css: &str) -> StyleTree {
    Stylesheets::parse(&[Source::author("decoration.css", css)]).compile(book, registry())
}

fn paginate(book: &Book, css: &str) -> Vec<Page> {
    let styles = styles(book, css);
    Paginator::new(registry(), &styles).paginate(book)
}

/// Where the run holding `words` sits: its left edge, its baseline,
/// and its size.
fn run_at(page: &Page, words: &str) -> (f32, f32, f32) {
    page.items
        .iter()
        .find_map(|item| match item {
            DrawItem::Text {
                x, y, size, text, ..
            } if text.contains(words) => Some((*x, *y, *size)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no run holding `{words}`: {:#?}", page.items))
}

/// One rule on a page: left edge, top edge, width, thickness, colour.
type Rule = (f32, f32, f32, f32, Color);

/// Every rule on a page, in paint order.
fn rules(page: &Page) -> Vec<Rule> {
    page.items
        .iter()
        .filter_map(|item| match item {
            DrawItem::Rect {
                x, y, w, h, color, ..
            } => Some((*x, *y, *w, *h, *color)),
            _ => None,
        })
        .collect()
}

/// The one rule a page carries.
fn one_rule(page: &Page) -> Rule {
    let rules = rules(page);
    assert_eq!(rules.len(), 1, "expected one rule, got {rules:?}");
    rules[0]
}

fn close(got: f32, want: f32, what: &str) {
    assert!((got - want).abs() < 1e-3, "{what}: got {got}, want {want}");
}

/// Acceptance: `a { text-decoration: underline }` draws a rule under
/// the link, at the offset and the thickness the face declares.
///
/// The bundled family is 1000 units to the em, and puts the top of
/// its underline 100 units under the baseline, 50 units thick.
#[test]
fn an_underline_sits_where_the_face_puts_it() {
    let book = linked("the whole of it");
    let pages = paginate(
        &book,
        &format!("{PAGE_CSS}\na {{ text-decoration: underline }}"),
    );
    let (x, baseline, size) = run_at(&pages[0], "the whole of it");
    let (rule_x, top, width, thickness, color) = one_rule(&pages[0]);
    close(rule_x, x, "the rule does not start at the run");
    close(
        top,
        baseline + size * 0.1,
        "the rule is not at the face's offset",
    );
    close(
        thickness,
        size * 0.05,
        "the rule is not the face's thickness",
    );
    assert!(width > 0.0, "the rule has no width");
    assert_eq!(color, Color::BLACK, "the rule lost the colour of the text");

    // The prose around the link takes no rule of its own, so the
    // rule starts after the words before the link.
    let (prose_x, ..) = run_at(&pages[0], "He read ");
    assert!(
        rule_x > prose_x,
        "the rule reached the prose before the link",
    );
}

/// Acceptance: a run broken across two lines draws a rule on both,
/// each as wide as the text on its own line.
#[test]
fn a_rule_broken_across_two_lines_draws_on_both() {
    let book = linked("the extraordinarily inconsiderate correspondence of the shipping office");
    let pages = paginate(
        &book,
        "@page { size: 160pt 300pt; margin: 12pt }\na { text-decoration: underline }",
    );
    let rules = rules(&pages[0]);
    assert!(
        rules.len() >= 2,
        "the rule drew on one line only: {rules:?}"
    );
    let tops: Vec<f32> = rules.iter().map(|rule| rule.1).collect();
    assert!(
        tops.windows(2).all(|pair| pair[1] > pair[0]),
        "the rules do not go down the page: {tops:?}",
    );
    for (x, _, width, _, _) in &rules {
        assert!(*width > 0.0, "a rule has no width: {rules:?}");
        assert!(
            x + width <= 160.0 - 12.0 + 1e-3,
            "a rule ran past the measure: {rules:?}",
        );
    }
    let widths: Vec<f32> = rules.iter().map(|rule| rule.2).collect();
    assert!(
        widths
            .windows(2)
            .any(|pair| (pair[0] - pair[1]).abs() > 1.0),
        "every line drew a rule of the same width: {widths:?}",
    );
}

/// Acceptance: `text-decoration-color` draws the rule in a colour
/// the text is not set in, and without one the rule takes the
/// colour of the text.
#[test]
fn a_rule_takes_a_colour_of_its_own() {
    let book = linked("the whole of it");
    let coloured = format!(
        "{PAGE_CSS}\na {{ color: #222222; text-decoration: underline; \
         text-decoration-color: rgb(180, 30, 30) }}"
    );
    let pages = paginate(&book, &coloured);
    let ink = pages[0]
        .items
        .iter()
        .find_map(|item| match item {
            DrawItem::Text { color, text, .. } if text.contains("the whole of it") => Some(*color),
            _ => None,
        })
        .expect("the link is set on the page");
    assert_eq!(ink, Color::rgb(34, 34, 34));
    assert_eq!(one_rule(&pages[0]).4, Color::rgb(180, 30, 30));

    let plain = format!("{PAGE_CSS}\na {{ color: #222222; text-decoration: underline }}");
    let inherited = paginate(&book, &plain);
    assert_eq!(one_rule(&inherited[0]).4, Color::rgb(34, 34, 34));
}

/// Acceptance: a struck run draws a line through it with no rule in
/// the sheet. The built-in stylesheet draws it.
#[test]
fn a_struck_run_draws_with_nothing_in_the_sheet() {
    let book = book_of(vec![
        text("He was "),
        Inline::Strikethrough {
            id: NodeId::UNASSIGNED,
            children: vec![text("certain")],
            attributes: Attributes::default(),
            position: None,
            span: None,
        },
        text(" almost certain."),
    ]);
    let pages = paginate(&book, PAGE_CSS);
    let (x, baseline, size) = run_at(&pages[0], "certain");
    let (rule_x, top, width, thickness, _) = one_rule(&pages[0]);
    close(rule_x, x, "the rule does not start at the run");
    // The bundled family puts the top of its strikeout 240 units
    // over the baseline, 50 units thick.
    close(
        top,
        baseline - size * 0.24,
        "the rule is not at the face's offset",
    );
    close(
        thickness,
        size * 0.05,
        "the rule is not the face's thickness",
    );
    assert!(width > 0.0, "the rule has no width");
}

/// Acceptance: two faces with different underline metrics draw at
/// different offsets. The second family is 2048 units to the em and
/// puts the top of its underline 158 units under the baseline, 84
/// units thick.
#[test]
fn two_faces_draw_at_their_own_offsets() {
    let book = linked("the whole of it");
    let under = |family: &str| {
        let css = format!(
            "{PAGE_CSS}\nbook {{ font-size: 12pt }}\n\
             a {{ font-family: {family}; text-decoration: underline }}"
        );
        let pages = paginate(&book, &css);
        let (_, baseline, _) = run_at(&pages[0], "the whole of it");
        let rule = one_rule(&pages[0]);
        (rule.1 - baseline, rule.3)
    };
    let (bundled_top, bundled_thickness) = under("serif");
    let (second_top, second_thickness) = under("\"IM Fell English SC\"");
    close(bundled_top, 12.0 * 100.0 / 1000.0, "the bundled face");
    close(second_top, 12.0 * 158.0 / 2048.0, "the second face");
    close(bundled_thickness, 12.0 * 50.0 / 1000.0, "the bundled face");
    close(second_thickness, 12.0 * 84.0 / 2048.0, "the second face");
}

/// Part: `text-decoration-thickness` sets a thickness of its own,
/// and the rule stays at the offset the face declares.
#[test]
fn a_thickness_in_the_sheet_overrides_the_face() {
    let book = linked("the whole of it");
    let css = format!(
        "{PAGE_CSS}\nbook {{ font-size: 12pt }}\n\
         a {{ text-decoration: underline; text-decoration-thickness: 2pt }}"
    );
    let pages = paginate(&book, &css);
    let (_, baseline, _) = run_at(&pages[0], "the whole of it");
    let rule = one_rule(&pages[0]);
    close(rule.3, 2.0, "the sheet's thickness did not reach the rule");
    close(rule.1 - baseline, 1.2, "the thickness moved the rule");
}

/// Part: `text-decoration-style: double` draws each rule twice, one
/// thickness apart.
#[test]
fn a_double_rule_is_two_rules_one_thickness_apart() {
    let book = linked("the whole of it");
    let css = format!(
        "{PAGE_CSS}\nbook {{ font-size: 12pt }}\n\
         a {{ text-decoration: underline double }}"
    );
    let pages = paginate(&book, &css);
    let rules = rules(&pages[0]);
    assert_eq!(rules.len(), 2, "{rules:?}");
    let thickness = rules[0].3;
    close(rules[1].3, thickness, "the rules differ in thickness");
    close(
        rules[1].1 - rules[0].1,
        thickness * 2.0,
        "the rules are not one thickness apart",
    );
    close(rules[1].0, rules[0].0, "the rules do not start together");
    close(rules[1].2, rules[0].2, "the rules differ in width");
}

/// Part: `text-decoration-line` draws a rule over the text and a
/// rule through it as well, and `none` draws nothing.
#[test]
fn every_line_the_property_names_draws() {
    let book = linked("the whole of it");
    let css = format!(
        "{PAGE_CSS}\nbook {{ font-size: 12pt }}\n\
         a {{ text-decoration-line: underline overline line-through }}"
    );
    let pages = paginate(&book, &css);
    let (_, baseline, _) = run_at(&pages[0], "the whole of it");
    let tops: Vec<f32> = rules(&pages[0])
        .iter()
        .map(|rule| rule.1 - baseline)
        .collect();
    assert_eq!(tops.len(), 3, "{tops:?}");
    // Over the text, through it, and under it, in that order down
    // the page. The first two are above the baseline.
    assert!(tops[0] < tops[1], "{tops:?}");
    assert!(tops[1] < 0.0, "{tops:?}");
    close(tops[2], 1.2, "the underline is not at the face's offset");

    let css = format!("{PAGE_CSS}\na {{ text-decoration-line: none }}");
    let undecorated = paginate(&book, &css);
    assert!(rules(&undecorated[0]).is_empty(), "`none` drew a rule");
}

/// Part: a rule is painted after the glyphs of the run it crosses.
#[test]
fn a_rule_paints_after_the_glyphs_of_its_run() {
    let book = linked("the whole of it");
    let pages = paginate(
        &book,
        &format!("{PAGE_CSS}\na {{ text-decoration: underline }}"),
    );
    let items = &pages[0].items;
    let run = items
        .iter()
        .position(|item| matches!(item, DrawItem::Text { text, .. } if text.contains("the whole")))
        .expect("the link is set on the page");
    let rule = items
        .iter()
        .position(|item| matches!(item, DrawItem::Rect { .. }))
        .expect("the link took a rule");
    assert_eq!(rule, run + 1, "the rule does not follow its own run");
}
