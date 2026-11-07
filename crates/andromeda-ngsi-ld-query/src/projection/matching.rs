//! Matching a `pick`/`omit` term list against a rendered attribute key (clause 4.21).
//!
//! Shared by every renderer that narrows already-rendered JSON against a compiled [`ProjectionTerm`] list: `join=inline` nesting, `join=flat` narrowing, and the temporal API's Linked Entity narrowing (clause 4.5.23). None of them walks a typed entity, so none can call [`ProjectionTerm::matches_attribute`] without first resolving the key's expanded IRI, which this function does through an [`AttributeIriIndex`].

use crate::projection::{attribute_iri_index::AttributeIriIndex, term::ProjectionTerm};

/// The term in `terms` naming the attribute keyed `key`, if any. A key the index does not cover can still be matched when the term's own IRI is spelled exactly as the key, as with an entity whose attributes are keyed by expanded IRI.
#[must_use]
pub fn matching_term<'t>(key: &str, terms: &'t [ProjectionTerm], index: &AttributeIriIndex) -> Option<&'t ProjectionTerm> {
    matching_position(key, terms, index).and_then(|position| terms.get(position))
}

/// The position in `terms` of the term naming the attribute keyed `key`, matched as [`matching_term`] matches it.
#[must_use]
pub fn matching_position(key: &str, terms: &[ProjectionTerm], index: &AttributeIriIndex) -> Option<usize> {
    terms.iter().position(|term| match index.expanded(key) {
        Some(expanded) => term.matches_attribute(key, expanded),
        None => matches!(term, ProjectionTerm::Attribute { iri, .. } if iri.as_str() == key),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn attribute_name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    fn leaf(name: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(name),
            linked: Vec::new(),
        }
    }

    /// A key the index covers matches the term whose IRI the index resolves it to.
    #[test]
    fn a_covered_key_matches_by_its_indexed_expansion() {
        let mut index = AttributeIriIndex::new();
        index.insert(attribute_name("category"), iri("category"));
        let terms = [leaf("category")];

        assert_eq!(matching_term("category", &terms, &index), Some(&terms[0]));
    }

    /// A key the index does not cover still matches a term whose IRI is spelled exactly as the key.
    #[test]
    fn an_uncovered_key_matches_a_term_spelled_as_the_expanded_iri() {
        let index = AttributeIriIndex::new();
        let terms = [leaf("category")];

        assert_eq!(matching_term("https://example.com/category", &terms, &index), Some(&terms[0]));
    }

    /// A key that names nothing in the term list matches nothing.
    #[test]
    fn an_unmatched_key_resolves_to_no_term() {
        let mut index = AttributeIriIndex::new();
        index.insert(attribute_name("name"), iri("name"));
        let terms = [leaf("category")];

        assert!(matching_term("name", &terms, &index).is_none());
    }

    /// The position is that of the first term naming the key.
    #[test]
    fn a_matching_position_counts_from_the_start_of_the_list() {
        let mut index = AttributeIriIndex::new();
        index.insert(attribute_name("name"), iri("name"));
        let terms = [ProjectionTerm::Id, leaf("category"), leaf("name")];

        assert_eq!(matching_position("name", &terms, &index), Some(2));
        assert_eq!(matching_position("size", &terms, &index), None);
    }
}
