//! Expansion of compact entity type names to IRIs (clause 4.17).

use crate::{
    name_stage::{Expanded, Written},
    query_error::QueryError,
    term_expander::TermExpander,
    type_selection::{TypeSelection, type_alternatives::TypeAlternatives, type_selection_error::TypeSelectionError},
};
use andromeda_ngsi_ld_lexicon::entity_type_iri::EntityTypeIri;

/// Expand a written type selection, replacing every compact type name with its
/// fully expanded IRI.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a type name does not expand
/// through the active `@context`, carrying
/// [`TermExpansionError::NotAnIri`](crate::term_expander::TermExpansionError::NotAnIri)
/// when it expands to something that is not an absolute IRI.
pub async fn expand_type_selection(type_selection: &TypeSelection<Written>, expander: &impl TermExpander) -> Result<TypeSelection<Expanded>, QueryError> {
    let TypeSelection::Alternatives(alternatives) = type_selection else {
        return Ok(TypeSelection::Wildcard);
    };
    let mut resolved = Vec::new();
    for conjunction in alternatives.iter() {
        let mut types = Vec::with_capacity(conjunction.len());
        for name in conjunction {
            types.push(EntityTypeIri::from(expander.expand_type(name.as_str()).await?));
        }
        resolved.push(types);
    }
    TypeAlternatives::new(resolved)
        .map(TypeSelection::Alternatives)
        .ok_or(QueryError::TypeSelection(TypeSelectionError::Empty))
}

#[cfg(test)]
mod tests {
    use crate::{
        name_stage::{Expanded, Written},
        namespace_expander::EXAMPLE_NAMESPACE_TYPED,
        type_selection::{TypeSelection, expansion::expand_type_selection},
    };
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn a_nested_type_selection_expands_at_every_leaf() {
        let type_selection: TypeSelection<Written> = "(Home;Vehicle),Motorhome".parse().expect("valid type expression");
        let expanded = expand_type_selection(&type_selection, &EXAMPLE_NAMESPACE_TYPED).await.expect("expands");
        let expected: TypeSelection<Expanded> = "(https://example.com/types/Home;https://example.com/types/Vehicle),https://example.com/types/Motorhome"
            .parse::<TypeSelection<Written>>()
            .expect("valid type expression")
            .into_expanded()
            .expect("absolute IRIs");
        assert_eq!(expanded, expected);
    }

    #[tokio::test]
    async fn the_wildcard_expands_to_the_wildcard() {
        let expanded = expand_type_selection(&TypeSelection::Wildcard, &EXAMPLE_NAMESPACE_TYPED)
            .await
            .expect("expands");
        assert_eq!(expanded, TypeSelection::Wildcard);
    }
}
