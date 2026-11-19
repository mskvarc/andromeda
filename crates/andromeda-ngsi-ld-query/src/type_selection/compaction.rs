//! Compaction of expanded entity type IRIs back to terms (clause 4.17).

use crate::{
    name_stage::{Expanded, Written},
    term_compactor::TermCompactor,
    type_selection::{TypeSelection, type_alternatives::TypeAlternatives},
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;

/// Compact an expanded type selection, replacing every type IRI with the term
/// the compactor's `@context` spells it with.
///
/// Each type is compacted on its own: `A,B` comes back as the two terms, never
/// as one name for the whole expression.
///
/// # Errors
///
/// Returns [`NgsiLdReferenceError::TooLong`] when no term abbreviates a type and
/// its IRI is longer than a written name may be.
pub async fn compact_type_selection(
    type_selection: &TypeSelection<Expanded>,
    compactor: &impl TermCompactor,
) -> Result<TypeSelection<Written>, NgsiLdReferenceError> {
    let TypeSelection::Alternatives(alternatives) = type_selection else {
        return Ok(TypeSelection::Wildcard);
    };
    let mut spelled = Vec::new();
    for conjunction in alternatives.iter() {
        let mut names = Vec::with_capacity(conjunction.len());
        for iri in conjunction {
            names.push(compactor.compact_type(iri).await?);
        }
        spelled.push(names);
    }
    TypeAlternatives::new(spelled)
        .map(TypeSelection::Alternatives)
        .ok_or(NgsiLdReferenceError::Empty)
}

#[cfg(test)]
mod tests {
    use crate::{
        name_stage::{Expanded, Written},
        term_compactor::TermCompactor,
        type_selection::{TypeSelection, compaction::compact_type_selection},
    };
    use andromeda_ngsi_ld_lexicon::{
        attribute_iri::AttributeIri,
        attribute_name::AttributeName,
        entity_type_iri::EntityTypeIri,
        entity_type_name::EntityTypeName,
        reference::NgsiLdReferenceError,
    };
    use std::future::{Future, ready};

    const TYPE_PREFIX: &str = "https://example.com/types/";

    /// Strips the type prefix and answers any other IRI as given.
    struct PrefixCompactor;

    impl TermCompactor for PrefixCompactor {
        fn compact_term(&self, name: &AttributeIri) -> impl Future<Output = AttributeName> + Send {
            ready(name.as_str().parse().expect("a valid attribute name"))
        }

        fn compact_type(&self, name: &EntityTypeIri) -> impl Future<Output = Result<EntityTypeName, NgsiLdReferenceError>> + Send {
            ready(name.as_str().strip_prefix(TYPE_PREFIX).unwrap_or(name.as_str()).parse())
        }
    }

    fn stored(text: &str) -> TypeSelection<Expanded> {
        text.parse::<TypeSelection<Written>>()
            .expect("valid type expression")
            .into_expanded()
            .expect("absolute IRIs")
    }

    #[tokio::test]
    async fn every_type_of_a_selection_is_compacted() {
        let compacted = compact_type_selection(&stored("https://example.com/types/A,https://example.com/types/B"), &PrefixCompactor)
            .await
            .expect("compacts");
        pretty_assertions::assert_eq!(compacted.to_string(), "A,B");
    }

    #[tokio::test]
    async fn the_types_of_a_conjunction_are_compacted_one_by_one() {
        let compacted = compact_type_selection(
            &stored("(https://example.com/types/A;https://example.com/types/B),https://other.example/C"),
            &PrefixCompactor,
        )
        .await
        .expect("compacts");
        pretty_assertions::assert_eq!(compacted.to_string(), "(A;B),https://other.example/C");
    }

    #[tokio::test]
    async fn the_wildcard_compacts_to_the_wildcard() {
        let compacted = compact_type_selection(&TypeSelection::Wildcard, &PrefixCompactor).await.expect("compacts");
        pretty_assertions::assert_eq!(compacted, TypeSelection::Wildcard);
    }
}
