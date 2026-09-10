//! What a host asks for: the pages, a PDF, and the folios one node
//! was set on.

use std::ops::Range;

use crate::LayoutOutput;
use crate::content::NodeId;
use crate::fonts::FontRegistry;
use crate::images::Assets;
use crate::layout::font_table;
use crate::pages::{DrawItem, Folios};
use crate::pdf::{self, PdfError};

use super::Session;

impl Session<'_> {
    /// The display structure, brought up to date.
    pub fn preview(&mut self) -> &LayoutOutput {
        self.update();
        self.output.as_ref().expect("an update leaves an output")
    }

    /// The same, as PDF bytes. The stages above the painter are the
    /// ones the preview used, so an export cannot contradict it.
    pub fn export(&mut self) -> Result<Vec<u8>, PdfError> {
        self.update();
        let output = self.output.as_ref().expect("an update leaves an output");
        pdf::write(
            output,
            self.registry.get(),
            self.assets.get(),
            &self.book.metadata,
        )
    }

    /// Where each of these nodes' content is set, answered in the
    /// order they were asked about: the folios it runs between, and
    /// the pages of the book those folios are.
    ///
    /// A node covers itself and everything under it, so a heading
    /// answers with the page its own text is on, and a chapter with
    /// the pages it runs across. Nothing for a node the book does
    /// not hold, and nothing for one whose content reaches no page:
    /// a node the engine synthesized, or a scene break, whose
    /// ornament the engine wrote itself.
    ///
    /// The answer is a walk over the pages the session already
    /// holds. It runs a stage only when an edit has left one to run.
    pub fn folios(&mut self, nodes: &[NodeId]) -> Vec<Option<Folios>> {
        let held: Vec<Option<Range<u32>>> =
            nodes.iter().map(|node| self.book.subtree(*node)).collect();
        self.update();
        let output = self.output.as_ref().expect("an update leaves an output");
        let mut answers: Vec<Option<Folios>> = vec![None; nodes.len()];
        for (at, page) in output.pages.iter().enumerate() {
            let at = at as u32;
            let mut reached = |node: NodeId| {
                for (answer, held) in answers.iter_mut().zip(&held) {
                    if !held.as_ref().is_some_and(|held| held.contains(&node.get())) {
                        continue;
                    }
                    *answer = Some(match *answer {
                        Some(folios) => Folios {
                            last: page.number,
                            count: at - folios.at + 1,
                            ..folios
                        },
                        None => Folios {
                            first: page.number,
                            last: page.number,
                            at,
                            count: 1,
                        },
                    });
                }
            };
            // A page names the sections it carries, and each run of
            // text names the node it was shaped from. Between them
            // they name every node the page took content from.
            for section in &page.sections {
                reached(*section);
            }
            for item in &page.items {
                if let DrawItem::Text {
                    origin: Some(origin),
                    ..
                } = item
                {
                    reached(origin.node);
                }
            }
        }
        answers
    }

    /// The display structure by value, consuming the session.
    pub fn into_output(mut self) -> LayoutOutput {
        self.update();
        self.output.take().expect("an update leaves an output")
    }
}

/// An output with the font and asset tables filled in and nothing
/// painted yet.
pub(super) fn blank_output(registry: &FontRegistry, assets: &Assets) -> LayoutOutput {
    LayoutOutput {
        pages: Vec::new(),
        fonts: font_table(registry),
        assets: assets.assets().to_vec(),
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::NodeId;

    use crate::session::Session;
    use crate::session::testing::{
        book, heading, named_on, prose, registry, section, sheets, three_chapters,
    };

    /// A chapter's content runs across pages, and the question
    /// answers the folio it opens on and the folio it ends on.
    #[test]
    fn a_node_answers_the_first_and_last_folio_it_is_set_on() {
        let mut session = three_chapters();
        let chapters: Vec<NodeId> = session.book().sections.iter().map(|s| s.id).collect();
        let settled = session.stages();
        let folios = session.folios(&chapters);
        assert_eq!(
            session.stages(),
            settled,
            "answering ran a stage over pages that were already placed"
        );
        let output = session.preview();

        for (chapter, folios) in chapters.iter().zip(&folios) {
            let named = named_on(output, *chapter);
            let at = *named.first().expect("a chapter of prose reaches a page");
            let last = *named.last().expect("a chapter of prose reaches a page");
            assert_eq!(
                *folios,
                Some(Folios {
                    first: output.pages[at].number,
                    last: output.pages[last].number,
                    at: at as u32,
                    count: (last - at + 1) as u32,
                }),
                "chapter {} is set on the pages {named:?}",
                chapter.get()
            );
        }
        assert!(
            folios
                .iter()
                .any(|folios| folios.is_some_and(|folios| folios.first < folios.last)),
            "no chapter of {} pages ran across two of them",
            output.pages.len()
        );
    }

    /// A folio is what a page has printed on it, and a book whose
    /// counter restarts prints a number that is not the page's place
    /// in the book. Both are answered, so a host names one and
    /// fetches by the other.
    #[test]
    fn a_restarted_counter_leaves_the_folio_and_the_page_apart() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![
            section("front.md", prose("alpha", 4)),
            section("body.md", prose("beta", 8)),
        ]));
        session.set_style(sheets("section:last-child { counter-reset: page 1 }"));
        let body = session.book().sections[1].id;
        let folios = session.folios(&[body])[0].expect("the chapter reaches a page");
        let output = session.preview();

        assert_eq!(folios.first, 1, "the restarted chapter opens at folio 1");
        assert!(
            folios.at > 0,
            "the restarted chapter is not the first page of the book"
        );
        assert_eq!(
            output.pages[folios.at as usize].number, folios.first,
            "`at` is not the page the folio is printed on"
        );
        assert_eq!(
            output.pages[(folios.at + folios.count - 1) as usize].number,
            folios.last,
            "`at` and `count` do not reach the folio it ends on"
        );
    }

    /// One call, one answer per node asked about, in the order they
    /// were asked about.
    #[test]
    fn several_nodes_are_answered_in_one_call() {
        let mut session = three_chapters();
        let mut asked: Vec<NodeId> = session.book().sections.iter().map(|s| s.id).collect();
        asked.reverse();
        asked.push(NodeId::new(u32::MAX));

        let together = session.folios(&asked);
        let apart: Vec<Option<Folios>> = asked
            .iter()
            .map(|node| session.folios(std::slice::from_ref(node))[0])
            .collect();
        assert_eq!(together, apart);
        assert_eq!(together.len(), asked.len());
    }

    /// A node the book does not hold, and the id the engine writes
    /// its own text under, are both answered with nothing rather
    /// than with a folio or an error.
    #[test]
    fn a_node_the_book_does_not_hold_answers_with_nothing() {
        let mut session = three_chapters();
        let past = NodeId::new(u32::MAX);
        assert_eq!(
            session.folios(&[past, NodeId::UNASSIGNED, past]),
            vec![None, None, None]
        );
    }

    /// A heading's runs are shaped from the text inside it, so no run
    /// names the heading itself. It answers with the page that text
    /// is on all the same.
    #[test]
    fn a_node_no_run_names_answers_with_the_page_its_content_is_on() {
        let mut session = Session::new(registry());
        session.set_content(book(vec![section(
            "one.md",
            [vec![heading("Chapter One")], prose("alpha", 8)].concat(),
        )]));
        let node = crate::content::block_id(&session.book().sections[0].blocks[0]);
        let folios = session.folios(&[node]);
        let output = session.preview();

        assert!(
            named_on(output, node).is_empty(),
            "a run named the heading, so this proves nothing"
        );
        let first = output.pages.first().expect("the book has pages").number;
        assert_eq!(
            folios,
            vec![Some(Folios {
                first,
                last: first,
                at: 0,
                count: 1,
            })]
        );
    }

    /// An export costs nothing the preview has not already paid: the
    /// stages above the painter are the same ones.
    #[test]
    fn export_paints_from_the_stages_the_preview_used() {
        let mut session = three_chapters();
        let before = session.stages();
        let bytes = session.export().expect("the fixture book writes PDF");
        assert_eq!(session.stages(), before, "an export re-ran a stage");
        assert!(bytes.starts_with(b"%PDF"), "that is not a PDF");
    }
}
