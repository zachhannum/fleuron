//! Reading a source twice gives the same tree, byte for byte.
//!
//! Layout is a pure function of the tree, so every downstream
//! guarantee rests on the reading in front of it: a stable page count,
//! and a PDF whose digest is checked in.

use fleuron::content::Metadata;
use fleuron_markdown::{Dialect, Options, Sections, assemble, to_sections};
use proptest::prelude::*;

/// The constructs a manuscript is built out of, plus the ones the
/// vocabulary has no room for: a mapping that degrades has more to be
/// unstable about than one that does not.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("# Part\n".to_string()),
        Just("## Chapter\n".to_string()),
        Just("### Scene\n".to_string()),
        Just("Plain prose, wrapped\nacross two lines.\n".to_string()),
        Just("Prose with _emphasis_, **strength** and `code`.\n".to_string()),
        Just("> Quoted.\n>\n> Still quoted.\n".to_string()),
        Just("---\n".to_string()),
        Just("- one\n- two\n".to_string()),
        Just("```\ncode line\n```\n".to_string()),
        Just("| a | b |\n|---|---|\n| c | d |\n".to_string()),
        Just("A [link](there.md) and an ![image](plate.png).\n".to_string()),
        Just("Text ~~struck~~ through.\n".to_string()),
    ]
}

fn options() -> impl Strategy<Value = Options> {
    (
        prop_oneof![
            Just(Sections::Whole),
            (1u8..=6).prop_map(|level| Sections::AtHeading(
                level.try_into().expect("1-6 is a heading level")
            ))
        ],
        prop_oneof![
            Just(Dialect::common_mark()),
            Just(Dialect::gfm()),
            Just(Dialect::obsidian())
        ],
    )
        .prop_map(|(sections, dialect)| Options { sections, dialect })
}

proptest! {
    #[test]
    fn reading_a_source_twice_gives_the_same_tree(
        fragments in prop::collection::vec(fragment(), 0..12),
        options in options(),
    ) {
        let markdown = fragments.concat();
        let read = || {
            let (sections, warnings) = to_sections(&markdown, "manuscript.md", &options);
            let book = assemble(Metadata::default(), sections);
            (
                serde_json::to_string(&book).expect("a content tree serializes"),
                format!("{warnings:?}"),
            )
        };
        prop_assert_eq!(read(), read());
    }
}
