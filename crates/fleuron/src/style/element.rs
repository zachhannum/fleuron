//! The content tree as something selectors can match against.
//!
//! `selectors` walks a DOM through parent and sibling links the
//! content tree does not have, so compilation flattens the tree once
//! into an arena that does. Element names are the markdown vocabulary
//! spelled the way an author writes them in CSS: `book`, `section`,
//! `h1`…`h6`, `p`, `blockquote`, `hr`, `img`, `em`, `strong`, `code`,
//! `a`.
//!
//! Text runs are not elements, the same as in CSS: they have no style
//! of their own and never count towards `:first-child`.

use std::borrow::Borrow;
use std::fmt;

use cssparser::{ToCss, serialize_identifier};
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{ElementSelectorFlags, MatchingContext};
use selectors::parser::{NonTSPseudoClass, PseudoElement as PseudoElementTrait, SelectorImpl};
use selectors::{Element, OpaqueElement};

use crate::content::{Block, Book, Inline, NodeId};

/// An interned CSS identifier: element name, class, id, namespace.
///
/// One type serves every slot `SelectorImpl` asks for; the novel
/// subset has no namespaces and no attributes, so the distinctions
/// the trait draws between them do not pay for separate types.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct Atom(pub String);

impl From<&str> for Atom {
    fn from(value: &str) -> Self {
        Atom(value.to_string())
    }
}

impl From<String> for Atom {
    fn from(value: String) -> Self {
        Atom(value)
    }
}

impl Borrow<str> for Atom {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl ToCss for Atom {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        serialize_identifier(&self.0, dest)
    }
}

impl PrecomputedHash for Atom {
    fn precomputed_hash(&self) -> u32 {
        // FNV-1a: the bloom filter needs a hash that is a function of
        // the name, not of where the string happens to live.
        let mut hash = 0x811c_9dc5u32;
        for byte in self.0.as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }
}

/// The novel subset has no non-tree-structural pseudo-classes:
/// `:hover` and its relatives describe a document being interacted
/// with, and a book is not one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PseudoClass {}

impl ToCss for PseudoClass {
    fn to_css<W: fmt::Write>(&self, _dest: &mut W) -> fmt::Result {
        match *self {}
    }
}

impl NonTSPseudoClass for PseudoClass {
    type Impl = Fleuron;

    fn is_active_or_hover(&self) -> bool {
        match *self {}
    }

    fn is_user_action_state(&self) -> bool {
        match *self {}
    }
}

/// The pseudo-elements the engine styles: the initial letter a drop
/// cap is set from, and nothing else.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PseudoElement {
    /// `::first-letter`
    FirstLetter,
}

impl ToCss for PseudoElement {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        match self {
            PseudoElement::FirstLetter => dest.write_str("::first-letter"),
        }
    }
}

impl PseudoElementTrait for PseudoElement {
    type Impl = Fleuron;
}

/// The engine's selector flavour.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fleuron;

impl SelectorImpl for Fleuron {
    type ExtraMatchingData<'a> = std::marker::PhantomData<&'a ()>;
    type AttrValue = Atom;
    type Identifier = Atom;
    type LocalName = Atom;
    type NamespaceUrl = Atom;
    type NamespacePrefix = Atom;
    type BorrowedLocalName = Atom;
    type BorrowedNamespaceUrl = Atom;
    type NonTSPseudoClass = PseudoClass;
    type PseudoElement = PseudoElement;
}

/// One element: a name, an identity in the content tree, and the
/// links a selector walks.
#[derive(Debug)]
pub struct ElementNode {
    /// The element name as CSS spells it.
    pub name: &'static str,
    /// The content node this element stands for.
    pub id: NodeId,
    pub parent: Option<usize>,
    pub previous: Option<usize>,
    pub next: Option<usize>,
    pub first_child: Option<usize>,
    /// True for elements with text of their own.
    pub has_text: bool,
}

/// The content tree flattened into an arena, in document order. Index
/// 0 is the book.
#[derive(Debug, Default)]
pub struct ElementTree {
    nodes: Vec<ElementNode>,
}

impl ElementTree {
    /// Flattens one book. Elements come out in document order, so
    /// index order is reading order.
    pub fn build(book: &Book) -> ElementTree {
        let mut tree = ElementTree::default();
        let root = tree.push("book", NodeId::UNASSIGNED, None, false);
        let sections: Vec<usize> = book
            .sections
            .iter()
            .map(|section| {
                let index = tree.push("section", section.id, Some(root), false);
                let children = tree.blocks(&section.blocks, index);
                tree.link(index, &children);
                index
            })
            .collect();
        tree.link(root, &sections);
        tree
    }

    /// Every element, in document order.
    pub fn nodes(&self) -> &[ElementNode] {
        &self.nodes
    }

    /// A handle onto one element, for matching.
    pub fn at(&self, index: usize) -> ElementRef<'_> {
        ElementRef { tree: self, index }
    }

    fn blocks(&mut self, blocks: &[Block], parent: usize) -> Vec<usize> {
        blocks
            .iter()
            .map(|block| match block {
                Block::Heading {
                    id, level, inlines, ..
                } => {
                    let name = match u8::from(*level) {
                        1 => "h1",
                        2 => "h2",
                        3 => "h3",
                        4 => "h4",
                        5 => "h5",
                        _ => "h6",
                    };
                    let index = self.push(name, *id, Some(parent), false);
                    let (children, has_text) = self.inlines(inlines, index);
                    self.link(index, &children);
                    self.nodes[index].has_text = has_text;
                    index
                }
                Block::Paragraph { id, inlines, .. } => {
                    let index = self.push("p", *id, Some(parent), false);
                    let (children, has_text) = self.inlines(inlines, index);
                    self.link(index, &children);
                    self.nodes[index].has_text = has_text;
                    index
                }
                Block::Blockquote { id, blocks, .. } => {
                    let index = self.push("blockquote", *id, Some(parent), false);
                    let children = self.blocks(blocks, index);
                    self.link(index, &children);
                    index
                }
                Block::ThematicBreak { id, .. } => self.push("hr", *id, Some(parent), false),
                Block::Image { id, .. } => self.push("img", *id, Some(parent), false),
            })
            .collect()
    }

    /// The element children of an inline sequence, plus whether any
    /// text run sits directly inside it — which is what `:empty` asks.
    fn inlines(&mut self, inlines: &[Inline], parent: usize) -> (Vec<usize>, bool) {
        let mut children = Vec::new();
        let mut text = false;
        for inline in inlines {
            let (name, id, nested): (_, _, Option<&[Inline]>) = match inline {
                Inline::Text { value, .. } => {
                    text |= !value.is_empty();
                    continue;
                }
                Inline::Code { id, value, .. } => {
                    let index = self.push("code", *id, Some(parent), !value.is_empty());
                    children.push(index);
                    continue;
                }
                Inline::Emphasis { id, children, .. } => ("em", *id, Some(children)),
                Inline::Strong { id, children, .. } => ("strong", *id, Some(children)),
                Inline::Link { id, children, .. } => ("a", *id, Some(children)),
            };
            let index = self.push(name, id, Some(parent), false);
            if let Some(nested) = nested {
                let (kids, has_text) = self.inlines(nested, index);
                self.link(index, &kids);
                self.nodes[index].has_text = has_text;
            }
            children.push(index);
        }
        (children, text)
    }

    fn push(
        &mut self,
        name: &'static str,
        id: NodeId,
        parent: Option<usize>,
        has_text: bool,
    ) -> usize {
        self.nodes.push(ElementNode {
            name,
            id,
            parent,
            previous: None,
            next: None,
            first_child: None,
            has_text,
        });
        self.nodes.len() - 1
    }

    /// Chains a parent's children: first child, and each one's
    /// siblings.
    fn link(&mut self, parent: usize, children: &[usize]) {
        self.nodes[parent].first_child = children.first().copied();
        for (position, index) in children.iter().enumerate() {
            self.nodes[*index].previous = position.checked_sub(1).map(|p| children[p]);
            self.nodes[*index].next = children.get(position + 1).copied();
        }
    }
}

/// One element of the arena, as `selectors` sees it.
#[derive(Clone, Copy)]
pub struct ElementRef<'a> {
    tree: &'a ElementTree,
    /// Index into the arena, which is also document order.
    pub index: usize,
}

impl ElementRef<'_> {
    fn node(&self) -> &ElementNode {
        &self.tree.nodes[self.index]
    }

    fn sibling(&self, which: fn(&ElementNode) -> Option<usize>) -> Option<Self> {
        which(self.node()).map(|index| ElementRef {
            tree: self.tree,
            index,
        })
    }
}

impl fmt::Debug for ElementRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.node().name)
    }
}

impl Element for ElementRef<'_> {
    type Impl = Fleuron;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.node())
    }

    fn parent_element(&self) -> Option<Self> {
        self.sibling(|node| node.parent)
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(|node| node.previous)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(|node| node.next)
    }

    fn first_element_child(&self) -> Option<Self> {
        self.sibling(|node| node.first_child)
    }

    fn is_html_element_in_html_document(&self) -> bool {
        false
    }

    fn has_local_name(&self, name: &Atom) -> bool {
        self.node().name == name.0
    }

    fn has_namespace(&self, ns: &Atom) -> bool {
        ns.0.is_empty()
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.node().name == other.node().name
    }

    fn attr_matches(
        &self,
        _ns: &NamespaceConstraint<&Atom>,
        _local_name: &Atom,
        _operation: &AttrSelectorOperation<&Atom>,
    ) -> bool {
        false
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &PseudoClass,
        _context: &mut MatchingContext<Fleuron>,
    ) -> bool {
        match *pc {}
    }

    /// Nothing in the content tree *is* a pseudo-element: a rule that
    /// names one is matched against its originating element instead,
    /// in `MatchingMode::ForStatelessPseudoElement`.
    fn match_pseudo_element(
        &self,
        _pe: &PseudoElement,
        _context: &mut MatchingContext<Fleuron>,
    ) -> bool {
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        self.node().name == "a"
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, _id: &Atom, _case_sensitivity: CaseSensitivity) -> bool {
        false
    }

    fn has_class(&self, _name: &Atom, _case_sensitivity: CaseSensitivity) -> bool {
        false
    }

    fn has_custom_state(&self, _name: &Atom) -> bool {
        false
    }

    fn imported_part(&self, _name: &Atom) -> Option<Atom> {
        None
    }

    fn is_part(&self, _name: &Atom) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.node().first_child.is_none() && !self.node().has_text
    }

    fn is_root(&self) -> bool {
        self.node().parent.is_none()
    }

    fn add_element_unique_hashes(&self, _filter: &mut BloomFilter) -> bool {
        false
    }
}
