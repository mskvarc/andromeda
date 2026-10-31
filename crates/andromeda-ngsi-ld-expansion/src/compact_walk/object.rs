//! Expanding one JSON object: a value object, a list or set object, or a node
//! object (JSON-LD 1.1 expansion algorithm, steps 9 to 20).

use crate::{
    compact_walk::{
        element::{Expanded, expand_element},
        member::expand_member,
        value_object::expand_value_object,
    },
    decline::Decline,
    expanded_tree::{ExpandedItem, ExpandedNode},
    term_scope::{ExpandedTerm, TermScope, id_text, identifier_text},
};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeMembers, NodeShape};
use iri_rs::IriBuf;
use jsonld::{Id, Term, syntax::Keyword};
use rdfx::BlankIdBuf;
use std::borrow::Cow;

/// One member of an object with its key expanded.
pub struct KeyedEntry<'k, N> {
    /// The key as written.
    pub key: Cow<'k, str>,
    /// What the key expanded to.
    pub term: ExpandedTerm,
    /// The value as written.
    pub value: N,
}

/// Expand an object written as the value of `active_property`, in the scope
/// already switched to the property-scoped context of `active_property`.
///
/// # Errors
///
/// Declines whatever the walk does not model.
pub fn expand_object<'n, N: CompactNode<'n>>(scope: &TermScope<'_>, active_property: Option<&str>, members: NodeMembers<'n, N>) -> Result<Expanded, Decline> {
    let mut entries = Vec::with_capacity(members.len());
    for (key, value) in members {
        let term = scope.expand_key(&key)?;
        match term.as_ref() {
            Term::Keyword(Keyword::Context) => return Err(Decline::EmbeddedContext),
            Term::Keyword(Keyword::Type) if declares_type_scoped_context(scope, value) => return Err(Decline::TypeScopedContext),
            Term::Null | Term::Id(_) | Term::Keyword(_) => {}
        }
        entries.push(KeyedEntry { key, term, value });
    }
    let keyword = |wanted: Keyword| {
        entries
            .iter()
            .find(|entry| matches!(entry.term.as_ref(), Term::Keyword(found) if *found == wanted))
    };
    if keyword(Keyword::Value).is_some() {
        return expand_value_object(scope, &entries);
    }
    if let (Some(list), Some(_)) = (keyword(Keyword::List), active_property) {
        if entries.len() != 1 {
            return Err(Decline::UnsupportedKeyword);
        }
        let mut items = Vec::new();
        for item in force_array(list.value) {
            items.extend(expand_element(scope, active_property, item)?.into_items());
        }
        scope.count_value()?;
        return Ok(Expanded::One(ExpandedItem::List(items)));
    }
    if let Some(set) = keyword(Keyword::Set) {
        if entries.len() != 1 {
            return Err(Decline::UnsupportedKeyword);
        }
        return expand_element(scope, active_property, set.value);
    }
    let mut node = NodeBuilder::default();
    for entry in &entries {
        node.add_json(scope, entry)?;
    }
    let finished = node.finish(active_property);
    if let Expanded::One(_) = finished {
        scope.count_value()?;
    }
    Ok(finished)
}

/// Whether one of the `@type` values names a term with a scoped context.
fn declares_type_scoped_context<'n, N: CompactNode<'n>>(scope: &TermScope<'_>, value: N) -> bool {
    force_array(value)
        .into_iter()
        .filter_map(CompactNode::text)
        .any(|term| scope.has_scoped_context(&term))
}

/// The items of a value read as an array (`[value]` for a non-array).
pub fn force_array<'n, N: CompactNode<'n>>(value: N) -> Vec<N> {
    match value.shape() {
        NodeShape::Array(items) => items,
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => vec![value],
    }
}

/// A node object being expanded, entry by entry (step 13).
#[derive(Default)]
pub struct NodeBuilder {
    /// The node so far.
    node: ExpandedNode,
    /// Whether an entry expanded to `@id` already.
    has_id: bool,
}

impl NodeBuilder {
    /// Add one entry written as JSON.
    ///
    /// # Errors
    ///
    /// Declines a keyword the walk does not model and a value the algorithm
    /// refuses.
    pub fn add_json<'n, N: CompactNode<'n>>(&mut self, scope: &TermScope<'_>, entry: &KeyedEntry<'_, N>) -> Result<(), Decline> {
        match entry.term.as_ref() {
            Term::Null => Ok(()),
            Term::Keyword(Keyword::Id) => {
                if self.has_id {
                    return Err(Decline::MalformedJsonLd);
                }
                let id = entry.value.text().ok_or(Decline::MalformedJsonLd)?;
                self.node.id = identifier_text(scope.expand_id(&id)?.as_ref());
                self.has_id = true;
                Ok(())
            }
            Term::Keyword(Keyword::Type) => {
                if declares_type_scoped_context(scope, entry.value) {
                    return Err(Decline::TypeScopedContext);
                }
                for type_name in force_array(entry.value) {
                    let type_name = type_name.text().ok_or(Decline::MalformedJsonLd)?;
                    match scope.expand_vocab(&type_name)?.as_ref() {
                        expanded @ Term::Id(_) => self.node.types.extend(identifier_text(expanded)),
                        Term::Null | Term::Keyword(_) => return Err(Decline::MalformedJsonLd),
                    }
                }
                Ok(())
            }
            Term::Keyword(_) => Err(Decline::UnsupportedKeyword),
            Term::Id(property) => {
                if let Some(name) = property_name(property)
                    && let Some(values) = expand_member(scope, &entry.key, entry.value)?
                {
                    self.node.add_member(name, values);
                }
                Ok(())
            }
        }
    }

    /// Add a member whose values were expanded already.
    pub fn add_expanded(&mut self, name: String, values: Vec<ExpandedItem>) {
        self.node.add_member(name, values);
    }

    /// The node, or nothing for a node JSON-LD drops: a free-floating one
    /// (written at the top of the document) holding nothing but an `@id`
    /// (step 20).
    #[must_use]
    pub fn finish(self, active_property: Option<&str>) -> Expanded {
        let ExpandedNode { types, members, .. } = &self.node;
        if active_property.is_none() && types.is_empty() && members.is_empty() {
            return Expanded::Null;
        }
        Expanded::One(ExpandedItem::Node(self.node))
    }
}

/// The member name a key that expanded to `property` names, or `None` for a
/// key expansion drops: one that expanded to text with no `:` (step 13.3).
pub fn property_name(property: &Id<IriBuf, BlankIdBuf>) -> Option<String> {
    let text = id_text(property);
    text.contains(':').then_some(text)
}
