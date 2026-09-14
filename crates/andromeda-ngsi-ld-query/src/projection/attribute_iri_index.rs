//! The expanded IRI of each attribute key a projection may be matched against.
//!
//! [`ProjectionTerm::matches_attribute`] needs an attribute key's expanded
//! IRI, and expanding a key needs the request `@context`. Anything that
//! applies a projection outside the layer that owns the JSON-LD engine — the
//! renderer applying a clause 4.21 nested projection while it nests linked
//! entities, above all — therefore needs that expansion handed to it already
//! resolved. This index is that hand-over.
//!
//! [`ProjectionTerm::matches_attribute`]: crate::projection::term::ProjectionTerm::matches_attribute

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::IriBuf;
use std::collections::HashMap;

/// Attribute keys mapped to the IRI they expand to under one request
/// `@context`.
///
/// A key absent from the index simply does not match any projection term: the
/// index is built from the keys the payload actually carries, so an absent key
/// is one no entity in the response holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeIriIndex {
    /// Each recorded attribute key and the IRI it expands to.
    expansions: HashMap<AttributeName, IriBuf>,
}

impl AttributeIriIndex {
    /// Create an empty index.
    #[must_use]
    pub fn new() -> AttributeIriIndex {
        AttributeIriIndex { expansions: HashMap::new() }
    }

    /// Record the IRI `key` expands to, replacing any earlier expansion of the
    /// same key.
    pub fn insert(&mut self, key: AttributeName, expanded: IriBuf) {
        self.expansions.insert(key, expanded);
    }

    /// The IRI `key` expands to, or `None` when the index does not cover it.
    #[must_use]
    pub fn expanded(&self, key: &str) -> Option<&IriBuf> {
        self.expansions.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// A recorded key resolves to the IRI it was recorded with, looked up by
    /// the plain key text a rendered value carries.
    #[test]
    fn a_recorded_key_resolves_to_its_expansion() {
        let mut index = AttributeIriIndex::new();
        index.insert(name("speed"), iri("https://example.com/speed"));

        assert_eq!(index.expanded("speed"), Some(&iri("https://example.com/speed")));
    }

    /// A key the index never saw resolves to nothing rather than to a guess.
    #[test]
    fn an_unrecorded_key_resolves_to_nothing() {
        let index = AttributeIriIndex::new();

        assert!(index.expanded("speed").is_none());
    }
}
