//! Compiling a query's entity selectors (clause 5.2.33) against the context.
//!
//! Shared by the entity and temporal query paths and the registration
//! preselect, so none of them compiles a selection differently: a POST
//! `entities[]` body is a union of independent selectors, and flattening it
//! into a comma-joined `id`/`type` pair would match a `(id, type)` pairing no
//! single selector named.

use crate::{
    expanded_entity_selector::ExpandedEntitySelector,
    query_error::QueryError,
    term_expander::TermExpander,
    type_selection::expansion::expand_type_expression,
    written_entity_selector::WrittenEntitySelector,
};

/// Compile a query's effective entity selectors through `expander`.
///
/// `selectors` is the union the query asks for (see
/// [`WrittenEntitySelector::effective`]): a POST `entities[]` body contributes
/// one per array entry, each compiled independently so their union is not
/// flattened into a cross product, and a GET request contributes its one
/// implicit selector.
///
/// # Errors
///
/// Reports [`QueryError`] when a selector's type expression has a term that
/// cannot be expanded through `expander`.
pub async fn compile_entity_selectors(selectors: &[WrittenEntitySelector], expander: &impl TermExpander) -> Result<Vec<ExpandedEntitySelector>, QueryError> {
    let mut compiled = Vec::with_capacity(selectors.len());
    for selector in selectors {
        let type_expr = match selector.type_selection.as_ref() {
            Some(raw) => Some(expand_type_expression(raw, expander).await?),
            None => None,
        };
        compiled.push(
            ExpandedEntitySelector::builder()
                .ids(selector.ids.clone().unwrap_or_default())
                .id_pattern(selector.id_pattern.clone())
                .type_selection(type_expr)
                .build(),
        );
    }
    Ok(compiled)
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace_expander::NamespaceExpander,
        pattern::Pattern,
        selector_compilation::compile_entity_selectors,
        type_selection::{expanded_type_selection::ExpandedTypeSelection, written_type_selection::WrittenTypeSelection},
        written_entity_selector::WrittenEntitySelector,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use iri_rs::iri;

    /// Expands every term and type under `https://example.org/`.
    const EXPANDER: NamespaceExpander = NamespaceExpander::uniform(iri!("https://example.org/"));

    fn identifier(text: &str) -> Identifier {
        text.parse().expect("a valid identifier")
    }

    fn type_iri(name: &str) -> ExpandedTypeSelection {
        ExpandedTypeSelection::Type(EXPANDER.type_iri(name).expect("a type resolves against the namespace"))
    }

    /// Clause 5.2.33: each selector of a union keeps its own identifiers,
    /// pattern and type, so no pairing across selectors is compiled.
    #[tokio::test]
    async fn each_selector_compiles_on_its_own() {
        let selectors = [
            WrittenEntitySelector {
                ids: Some(vec![identifier("urn:ngsi-ld:A:1")]),
                type_selection: Some("A".parse::<WrittenTypeSelection>().expect("a valid type")),
                ..WrittenEntitySelector::default()
            },
            WrittenEntitySelector {
                id_pattern: Some("^urn:ngsi-ld:B:.*".parse::<Pattern>().expect("a valid pattern")),
                type_selection: Some("B|C".parse::<WrittenTypeSelection>().expect("a valid type")),
                ..WrittenEntitySelector::default()
            },
        ];
        let compiled = compile_entity_selectors(&selectors, &EXPANDER).await.expect("the selectors compile");

        pretty_assertions::assert_eq!(compiled.len(), 2);
        pretty_assertions::assert_eq!(compiled[0].ids, vec![identifier("urn:ngsi-ld:A:1")]);
        pretty_assertions::assert_eq!(compiled[0].id_pattern, None);
        pretty_assertions::assert_eq!(compiled[0].type_selection, Some(type_iri("A")));
        pretty_assertions::assert_eq!(compiled[1].ids, Vec::<Identifier>::new());
        pretty_assertions::assert_eq!(compiled[1].id_pattern, selectors[1].id_pattern);
        pretty_assertions::assert_eq!(compiled[1].type_selection, Some(ExpandedTypeSelection::Or(vec![type_iri("B"), type_iri("C")])));
    }

    /// Clause 6.4.3.2 leaves `type` optional on a GET query, so a selector
    /// without one constrains no type.
    #[tokio::test]
    async fn a_selector_without_a_type_constrains_none() {
        let compiled = compile_entity_selectors(&[WrittenEntitySelector::default()], &EXPANDER)
            .await
            .expect("the selector compiles");
        pretty_assertions::assert_eq!(compiled.len(), 1);
        assert!(!compiled[0].constrains());
    }
}
