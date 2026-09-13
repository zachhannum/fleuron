//! What a link in the book reaches.
//!
//! A link names a source and a heading in it, the way an editor that
//! keeps one note per chapter writes it: `chapter-03.md#the-hunter`,
//! `Chapter%203.md#The%20Hunter`, or `[[Chapter 3#The Hunter]]`. A
//! heading anchor is not a CSS id. Two chapters can each have a
//! heading `Notes`, and a link reaches the one in the source it names.

use std::collections::{BTreeMap, BTreeSet};

use super::{Block, Book, Inline, NodeId, Section, block_attributes, block_id, inline_attributes};
use super::{inline_id, rows, text};

/// The characters a heading and a link both ignore when a link names
/// the heading by its text. They are the ones Obsidian ignores, and
/// `_`, which markup writes and the heading text leaves out.
const IGNORED: &str = "!\"#$%&()*+,.:;<=>?@^`{|}~/[]\\_";

/// What the links in one book can reach: each source, the headings
/// in it, and every id a source writes.
#[derive(Debug, Clone, Default)]
pub struct Anchors {
    sources: Vec<Source>,
    /// Every written id, over the whole book. The first node to take
    /// an id keeps it.
    written: BTreeMap<String, NodeId>,
    /// What a warning calls each node that a link can reach.
    names: BTreeMap<NodeId, String>,
}

/// What one link reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    /// A node of the book: a heading, a node with a written id, or the
    /// first section of a source.
    Node(NodeId),
    /// Something outside the book, such as `https://example.com`.
    Outside,
    /// A source or a heading that the book does not have.
    Missing,
}

/// The sections read from one source.
#[derive(Debug, Clone)]
struct Source {
    name: Option<String>,
    /// The name as a link matches it: lowercase, `/` between folders,
    /// and no `.md`.
    path: Option<String>,
    /// The section the source opens with.
    first: NodeId,
    written: BTreeMap<String, NodeId>,
    slugs: BTreeMap<String, NodeId>,
    headings: Vec<Heading>,
}

#[derive(Debug, Clone)]
struct Heading {
    node: NodeId,
    level: u8,
    /// The text as a link written in text form matches it.
    text: String,
    /// The slug before a count, where the source writes no id.
    slug: Option<String>,
}

impl Book {
    /// What the links in this book can reach.
    ///
    /// Read it after [`Book::assign_node_ids`], because the nodes it
    /// gives are the ids that numbering assigns.
    pub fn anchors(&self) -> Anchors {
        Anchors::of(self)
    }
}

impl Anchors {
    /// What the links in `book` can reach.
    pub fn of(book: &Book) -> Anchors {
        let mut anchors = Anchors::default();
        let mut index: BTreeMap<Option<&str>, usize> = BTreeMap::new();
        for section in &book.sections {
            let at = *index.entry(section.source.as_deref()).or_insert_with(|| {
                anchors.sources.push(Source::new(section));
                anchors.sources.len() - 1
            });
            let source = &mut anchors.sources[at];
            source.collect(&section.blocks);
        }
        for source in &mut anchors.sources {
            source.count_slugs();
            for (id, node) in &source.written {
                anchors.written.entry(id.clone()).or_insert(*node);
            }
        }
        let sources = std::mem::take(&mut anchors.sources);
        for source in &sources {
            anchors.name_source(source);
        }
        anchors.sources = sources;
        anchors
    }

    /// The node a link to `url` reaches, where the link is written in
    /// the source named `from`.
    ///
    /// The part of `url` before `#` names a source. It matches a
    /// source name with no regard to case, with or without `.md`, and
    /// with percent escapes decoded. A path that starts with `./` or
    /// `../` is relative to the folder of `from`. Any other path
    /// matches the end of a source name, and a source in the folder of
    /// `from` comes first. A link with no source part names `from`.
    ///
    /// The part after `#` names a node in that source: an id the
    /// source writes, the slug of a heading, or the text of a heading.
    /// Text matches with no regard to case or punctuation, and the
    /// first heading that matches wins. `#Part#Notes` names a heading
    /// `Notes` under a heading `Part`. A link with no source part also
    /// reaches an id that any source writes.
    ///
    /// A link with no `#` part reaches the first section of the source.
    pub fn resolve(&self, url: &str, from: Option<&str>) -> LinkTarget {
        if url.is_empty() {
            return LinkTarget::Missing;
        }
        if outside(url) {
            return LinkTarget::Outside;
        }
        let (path, fragment) = match url.split_once('#') {
            Some((path, fragment)) => (path, Some(fragment)),
            None => (url, None),
        };
        let from = self.sources.iter().position(|s| s.name.as_deref() == from);
        let source = if path.is_empty() {
            from
        } else {
            match self.find(&decode(path), from) {
                Some(source) => Some(source),
                None => return LinkTarget::Missing,
            }
        };
        let fragment = fragment.map(decode).filter(|fragment| !fragment.is_empty());
        let found = match &fragment {
            None if path.is_empty() => None,
            None => source.map(|source| self.sources[source].first),
            Some(fragment) => source
                .and_then(|source| self.sources[source].find(fragment))
                .or_else(|| {
                    path.is_empty()
                        .then(|| self.written.get(fragment).copied())
                        .flatten()
                }),
        };
        found.map_or(LinkTarget::Missing, LinkTarget::Node)
    }

    /// What a warning calls a node that a link can reach.
    pub(crate) fn name(&self, node: NodeId) -> Option<&str> {
        self.names.get(&node).map(String::as_str)
    }

    /// Whether a link can reach a node.
    pub(crate) fn reaches(&self, node: NodeId) -> bool {
        self.names.contains_key(&node)
    }

    fn find(&self, path: &str, from: Option<usize>) -> Option<usize> {
        let folder = from
            .and_then(|from| self.sources[from].path.as_deref())
            .and_then(|path| path.rsplit_once('/'))
            .map_or("", |(folder, _)| folder);
        let relative = path.starts_with("./") || path.starts_with("../");
        let rooted = path.starts_with('/');
        let wanted = match (relative, folder.is_empty()) {
            (true, false) => path_key(&format!("{folder}/{path}")),
            (true, true) => path_key(path),
            (false, _) => path_key(path.trim_start_matches('/')),
        };
        let named = || {
            self.sources
                .iter()
                .enumerate()
                .filter_map(|(at, source)| Some((at, source.path.as_deref()?)))
        };
        if let Some((at, _)) = named().find(|(_, path)| *path == wanted) {
            return Some(at);
        }
        if relative || rooted || wanted.is_empty() {
            return None;
        }
        let ending = format!("/{wanted}");
        let within = format!("{folder}/");
        let ends = || named().filter(|(_, path)| path.ends_with(&ending));
        ends()
            .find(|(_, path)| !folder.is_empty() && path.starts_with(&within))
            .or_else(|| ends().next())
            .map(|(at, _)| at)
    }

    fn name_source(&mut self, source: &Source) {
        let prefix = source.name.as_deref().unwrap_or_default();
        if let Some(name) = &source.name {
            self.names.insert(source.first, name.clone());
        }
        for (id, node) in &source.written {
            self.names.entry(*node).or_insert_with(|| id.clone());
        }
        for (slug, node) in &source.slugs {
            self.names
                .entry(*node)
                .or_insert_with(|| format!("{prefix}#{slug}"));
        }
        for heading in source.headings.iter().filter(|h| !h.text.is_empty()) {
            self.names
                .entry(heading.node)
                .or_insert_with(|| format!("{prefix}#{}", heading.text));
        }
    }
}

impl Source {
    fn new(section: &Section) -> Source {
        Source {
            name: section.source.clone(),
            path: section.source.as_deref().map(path_key),
            first: section.id,
            written: BTreeMap::new(),
            slugs: BTreeMap::new(),
            headings: Vec::new(),
        }
    }

    fn collect(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.write(block_id(block), &block_attributes(block).id);
            match block {
                Block::Heading {
                    id,
                    level,
                    inlines,
                    attributes,
                    ..
                } => {
                    let words = text(inlines);
                    self.headings.push(Heading {
                        node: *id,
                        level: u8::from(*level),
                        text: matched(&words),
                        slug: attributes.id.is_none().then(|| slug(&words)).flatten(),
                    });
                    self.collect_inlines(inlines);
                }
                Block::Paragraph { inlines, .. } => self.collect_inlines(inlines),
                Block::Blockquote { blocks, .. } => self.collect(blocks),
                Block::Table { head, body, .. } => {
                    for row in rows(head, body) {
                        self.write(row.id, &row.attributes.id);
                        for cell in &row.cells {
                            self.write(cell.id, &cell.attributes.id);
                            self.collect(&cell.blocks);
                        }
                    }
                }
                Block::ThematicBreak { .. } | Block::Image { .. } => {}
            }
        }
    }

    fn collect_inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            self.write(inline_id(inline), &inline_attributes(inline).id);
            if let Inline::Emphasis { children, .. }
            | Inline::Strong { children, .. }
            | Inline::Link { children, .. } = inline
            {
                self.collect_inlines(children);
            }
        }
    }

    fn write(&mut self, node: NodeId, id: &Option<String>) {
        if let Some(id) = id {
            self.written.entry(id.clone()).or_insert(node);
        }
    }

    /// Gives each heading with no written id its slug. A slug that an
    /// earlier heading or a written id in the source has takes `-2`,
    /// then `-3`, until it is free.
    fn count_slugs(&mut self) {
        let mut taken: BTreeSet<String> = self.written.keys().cloned().collect();
        for heading in &self.headings {
            let Some(base) = &heading.slug else {
                continue;
            };
            let mut slug = base.clone();
            let mut count = 2;
            while taken.contains(&slug) {
                slug = format!("{base}-{count}");
                count += 1;
            }
            taken.insert(slug.clone());
            self.slugs.insert(slug, heading.node);
        }
    }

    fn find(&self, fragment: &str) -> Option<NodeId> {
        if let Some(node) = self
            .written
            .get(fragment)
            .or_else(|| self.slugs.get(fragment))
        {
            return Some(*node);
        }
        let wanted: Vec<String> = fragment
            .split('#')
            .map(matched)
            .filter(|text| !text.is_empty())
            .collect();
        let mut wanted = wanted.iter();
        let mut next = wanted.next()?;
        let mut level = 0;
        for heading in &self.headings {
            if heading.level > level && heading.text == *next {
                level = heading.level;
                match wanted.next() {
                    Some(text) => next = text,
                    None => return Some(heading.node),
                }
            }
        }
        None
    }
}

/// The slug of a heading's text: lowercase, each run of characters
/// that are not letters or digits as one hyphen, and no hyphen at
/// either end. Text with no letter or digit has none.
fn slug(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    (!words.is_empty()).then(|| words.join("-"))
}

/// Text as a link in text form matches it: lowercase, each ignored
/// character as a space, and each run of spaces as one.
fn matched(text: &str) -> String {
    let spaced: String = text
        .chars()
        .map(|c| if IGNORED.contains(c) { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A source name or a link path as the two match: lowercase, `/`
/// between folders, `.` and `..` resolved, and no `.md`.
fn path_key(path: &str) -> String {
    let lower = path.replace('\\', "/").to_lowercase();
    let lower = lower.strip_suffix(".md").unwrap_or(&lower);
    let rooted = lower.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in lower.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let joined = parts.join("/");
    if rooted { format!("/{joined}") } else { joined }
}

/// Whether a url names something outside the book: it starts with a
/// scheme such as `https:` or `mailto:`, or with `//`.
fn outside(url: &str) -> bool {
    if url.starts_with("//") {
        return true;
    }
    let Some((scheme, _)) = url.split_once(':') else {
        return false;
    };
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

/// Text with each percent escape decoded. Text whose escapes do not
/// decode to UTF-8 stays as it is.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escape = bytes
            .get(at + 1..at + 3)
            .filter(|hex| bytes[at] == b'%' && hex.iter().all(u8::is_ascii_hexdigit));
        match escape {
            Some(hex) => {
                let hex = std::str::from_utf8(hex).expect("hex digits are ASCII");
                out.push(u8::from_str_radix(hex, 16).expect("two hex digits are a byte"));
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{Attributes, HeadingLevel};

    fn words(value: &str) -> Inline {
        Inline::Text {
            id: NodeId::UNASSIGNED,
            value: value.into(),
            attributes: Attributes::default(),
            position: None,
            span: None,
        }
    }

    fn heading_at(level: HeadingLevel, inlines: Vec<Inline>, written: Option<&str>) -> Block {
        Block::Heading {
            id: NodeId::UNASSIGNED,
            level,
            inlines,
            attributes: Attributes {
                id: written.map(Into::into),
                classes: Vec::new(),
            },
            position: None,
            span: None,
        }
    }

    fn h1(title: &str) -> Block {
        heading_at(HeadingLevel::H1, vec![words(title)], None)
    }

    fn h2(title: &str) -> Block {
        heading_at(HeadingLevel::H2, vec![words(title)], None)
    }

    fn source(name: Option<&str>, blocks: Vec<Block>) -> Section {
        Section {
            source: name.map(Into::into),
            blocks,
            ..Section::default()
        }
    }

    fn book(sections: Vec<Section>) -> Book {
        let mut book = Book {
            sections,
            ..Book::default()
        };
        book.assign_node_ids();
        book
    }

    /// The node of the `nth` heading of the book, counted from 0.
    fn heading(book: &Book, nth: usize) -> LinkTarget {
        let mut headings = Vec::new();
        fn each(blocks: &[Block], out: &mut Vec<NodeId>) {
            for block in blocks {
                match block {
                    Block::Heading { id, .. } => out.push(*id),
                    Block::Blockquote { blocks, .. } => each(blocks, out),
                    _ => {}
                }
            }
        }
        for section in &book.sections {
            each(&section.blocks, &mut headings);
        }
        LinkTarget::Node(headings[nth])
    }

    #[test]
    fn a_slug_is_the_text_lowercased_with_hyphens() {
        for (title, want) in [
            ("The Hunter", Some("the-hunter")),
            ("  Chapter 1: “Arrival”!  ", Some("chapter-1-arrival")),
            ("Café Noir", Some("café-noir")),
            ("— ? —", None),
            ("", None),
        ] {
            assert_eq!(slug(title).as_deref(), want, "{title:?}");
        }
    }

    /// Slugs count per source. A slug skips the written ids of its own
    /// source and not those of another.
    #[test]
    fn slugs_count_per_source() {
        let book = book(vec![
            source(Some("one.md"), vec![h1("Notes"), h1("Notes")]),
            source(
                Some("two.md"),
                vec![
                    h1("Notes"),
                    heading_at(HeadingLevel::H1, vec![words("Hunt")], Some("hunt")),
                    h1("Hunt"),
                ],
            ),
            source(Some("three.md"), vec![h1("Hunt")]),
        ]);
        let anchors = book.anchors();
        let at = |url, from| anchors.resolve(url, Some(from));
        assert_eq!(at("#notes", "one.md"), heading(&book, 0));
        assert_eq!(at("#notes-2", "one.md"), heading(&book, 1));
        assert_eq!(at("#notes", "two.md"), heading(&book, 2));
        assert_eq!(at("#notes-2", "two.md"), LinkTarget::Missing);
        assert_eq!(at("#hunt", "two.md"), heading(&book, 3));
        assert_eq!(at("#hunt-2", "two.md"), heading(&book, 4));
        assert_eq!(at("#hunt", "three.md"), heading(&book, 5));
    }

    /// Text form ignores case and punctuation. Markup is not part of
    /// the text, and the first heading that matches wins.
    #[test]
    fn text_form_matches_as_obsidian_does() {
        let book = book(vec![source(
            Some("note.md"),
            vec![
                heading_at(
                    HeadingLevel::H2,
                    vec![
                        words("What's "),
                        Inline::Emphasis {
                            id: NodeId::UNASSIGNED,
                            children: vec![words("new")],
                            attributes: Attributes::default(),
                            position: None,
                            span: None,
                        },
                        words("?"),
                    ],
                    None,
                ),
                h2("A: B / C"),
                h2("A B C"),
            ],
        )]);
        let anchors = book.anchors();
        for (url, nth) in [
            ("#What's *new*?", 0),
            ("#what's new", 0),
            ("#What's%20_new_", 0),
            ("#A B / C", 1),
            ("#a b c", 1),
            ("#a-b-c-2", 2),
        ] {
            assert_eq!(
                anchors.resolve(url, Some("note.md")),
                heading(&book, nth),
                "{url}"
            );
        }
        assert_eq!(
            anchors.resolve("#whats new", Some("note.md")),
            LinkTarget::Missing
        );
    }

    /// `#Part#Notes` names the first `Notes` deeper than the `Part`
    /// before it.
    #[test]
    fn a_nested_heading_follows_its_parents() {
        let book = book(vec![source(
            Some("note.md"),
            vec![
                h2("Notes"),
                h1("Part"),
                h2("Notes"),
                h1("Other"),
                h2("Notes"),
            ],
        )]);
        let anchors = book.anchors();
        let at = |url| anchors.resolve(url, Some("note.md"));
        assert_eq!(at("#Notes"), heading(&book, 0));
        assert_eq!(at("#Part#Notes"), heading(&book, 2));
        assert_eq!(at("#Other#Notes"), heading(&book, 4));
        assert_eq!(at("#Notes#Part"), LinkTarget::Missing);
    }

    #[test]
    fn a_file_matches_as_obsidian_finds_it() {
        let book = book(vec![
            source(Some("vault/part-1/Chapter 3.md"), vec![h1("One")]),
            source(Some("vault/part-2/Chapter 3.md"), vec![h1("Two")]),
            source(Some("vault/part-2/notes.md"), vec![h1("Notes")]),
            source(Some("vault/index.md"), vec![h1("Index")]),
        ]);
        let anchors = book.anchors();
        let at = |url, from| anchors.resolve(url, Some(from));
        let notes = "vault/part-2/notes.md";
        let index = "vault/index.md";
        for (url, from, nth) in [
            ("Chapter 3#One", index, 0),
            ("chapter 3.MD#one", index, 0),
            ("Chapter%203.md#One", index, 0),
            ("Chapter 3#Two", notes, 1),
            ("./Chapter 3.md#Two", notes, 1),
            ("../part-1/Chapter 3#One", notes, 0),
            ("part-2/chapter 3#two", index, 1),
            ("vault/part-2/Chapter 3#Two", index, 1),
            ("notes", index, 2),
        ] {
            let found = anchors.resolve(url, Some(from));
            match heading(&book, nth) {
                // A link with no `#` part reaches the section.
                _ if !url.contains('#') => {
                    assert_eq!(found, LinkTarget::Node(book.sections[nth].id), "{url}")
                }
                want => assert_eq!(found, want, "{url} from {from}"),
            }
        }
        assert_eq!(at("./Chapter 3#One", notes), LinkTarget::Missing);
        assert_eq!(at("/part-2/notes", index), LinkTarget::Missing);
        assert_eq!(at("elsewhere.md#One", index), LinkTarget::Missing);
    }

    /// A link with no `#` part reaches the first section of the
    /// source, when the source is cut into several.
    #[test]
    fn a_file_alone_reaches_its_first_section() {
        let book = book(vec![
            source(Some("one.md"), vec![h1("A")]),
            source(Some("two.md"), vec![h1("B")]),
            source(Some("two.md"), vec![h1("C")]),
        ]);
        let anchors = book.anchors();
        assert_eq!(
            anchors.resolve("two", Some("one.md")),
            LinkTarget::Node(book.sections[1].id)
        );
        assert_eq!(anchors.name(book.sections[1].id), Some("two.md"));
        assert!(!anchors.reaches(book.sections[2].id));
    }

    /// A link with no source part reaches an id written in another
    /// source, after the headings of its own.
    #[test]
    fn a_written_id_reaches_across_sources() {
        let chase = heading_at(HeadingLevel::H1, vec![words("Chase")], Some("hunt"));
        let book = book(vec![
            source(Some("one.md"), vec![chase]),
            source(Some("two.md"), vec![h1("Other")]),
            source(Some("three.md"), vec![h1("Hunt")]),
        ]);
        let anchors = book.anchors();
        assert_eq!(anchors.resolve("#hunt", Some("two.md")), heading(&book, 0));
        assert_eq!(
            anchors.resolve("#hunt", Some("three.md")),
            heading(&book, 2)
        );
        assert_eq!(
            anchors.resolve("two.md#hunt", Some("one.md")),
            LinkTarget::Missing
        );
        assert_eq!(
            anchors.name(block_id(&book.sections[0].blocks[0])),
            Some("hunt")
        );
        assert_eq!(
            anchors.name(block_id(&book.sections[2].blocks[0])),
            Some("three.md#hunt")
        );
    }

    #[test]
    fn a_url_with_a_scheme_is_outside() {
        let book = book(vec![source(None, vec![h1("A")])]);
        let anchors = book.anchors();
        for url in ["https://example.com", "mailto:a@b.c", "//example.com/x"] {
            assert_eq!(anchors.resolve(url, None), LinkTarget::Outside, "{url}");
        }
        for url in ["", "#", "#b", "a.md", "A B#c"] {
            assert_eq!(anchors.resolve(url, None), LinkTarget::Missing, "{url}");
        }
        assert_eq!(anchors.resolve("#a", None), heading(&book, 0));
    }

    #[test]
    fn a_percent_escape_decodes() {
        assert_eq!(decode("Chapter%203"), "Chapter 3");
        assert_eq!(decode("caf%C3%A9"), "café");
        assert_eq!(decode("100% sure%2"), "100% sure%2");
        assert_eq!(decode("%FF"), "%FF");
    }
}
