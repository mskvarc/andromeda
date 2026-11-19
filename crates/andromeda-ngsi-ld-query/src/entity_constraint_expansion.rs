//! Compiling a query's entity constraints (clause 5.2.33) against the context.
//!
//! Shared by the entity and temporal query paths and the registration
//! preselect, so none of them compiles a selection differently: a POST
//! `entities[]` body is a union of independent selectors, and flattening it
//! into a comma-joined `id`/`type` pair would match a `(id, type)` pairing no
//! single selector named.

use crate::{
    entity_constraint::EntityConstraint,
    name_stage::{Expanded, Written},
    query_error::QueryError,
    term_expander::TermExpander,
    type_selection::expansion::expand_type_selection,
};

/// Compile a query's effective entity constraints through `expander`.
///
/// `constraints` is the union the query asks for (see
/// [`EntityConstraint::effective`]): a POST `entities[]` body contributes one
/// per array entry, each compiled independently so their union is not
/// flattened into a cross product, and a GET request contributes its one
/// implicit selector.
///
/// # Errors
///
/// Reports [`QueryError`] when a constraint's type selection has a term that
/// cannot be expanded through `expander`.
pub async fn expand_entity_constraints(
    constraints: &[EntityConstraint<Written>],
    expander: &impl TermExpander,
) -> Result<Vec<EntityConstraint<Expanded>>, QueryError> {
    let mut compiled = Vec::with_capacity(constraints.len());
    for constraint in constraints {
        let type_selection = match constraint.type_selection.as_ref() {
            Some(type_selection) => Some(expand_type_selection(type_selection, expander).await?),
            None => None,
        };
        compiled.push(
            EntityConstraint::builder()
                .ids(constraint.ids.clone())
                .id_pattern(constraint.id_pattern.clone())
                .type_selection(type_selection)
                .build(),
        );
    }
    Ok(compiled)
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_constraint::EntityConstraint,
        entity_constraint_expansion::expand_entity_constraints,
        name_stage::{Expanded, Written},
        namespace_expander::NamespaceExpander,
        pattern::Pattern,
        type_selection::TypeSelection,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use iri_rs::iri;

    /// Expands every term and type under `https://example.org/`.
    const EXPANDER: NamespaceExpander = NamespaceExpander::uniform(iri!("https://example.org/"));

    fn identifier(text: &str) -> Identifier {
        text.parse().expect("a valid identifier")
    }

    fn type_iri(name: &str) -> TypeSelection<Expanded> {
        TypeSelection::single(EXPANDER.type_iri(name).expect("a type resolves against the namespace").into())
    }

    /// Clause 5.2.33: each selector of a union keeps its own identifiers,
    /// pattern and type, so no pairing across selectors is compiled.
    #[tokio::test]
    async fn each_selector_compiles_on_its_own() {
        let constraints = [
            EntityConstraint::<Written> {
                ids: vec![identifier("urn:ngsi-ld:A:1")],
                type_selection: Some("A".parse::<TypeSelection<Written>>().expect("a valid type")),
                ..EntityConstraint::default()
            },
            EntityConstraint::<Written> {
                id_pattern: Some("^urn:ngsi-ld:B:.*".parse::<Pattern>().expect("a valid pattern")),
                type_selection: Some("B|C".parse::<TypeSelection<Written>>().expect("a valid type")),
                ..EntityConstraint::default()
            },
        ];
        let compiled = expand_entity_constraints(&constraints, &EXPANDER).await.expect("the selectors compile");

        pretty_assertions::assert_eq!(compiled.len(), 2);
        pretty_assertions::assert_eq!(compiled[0].ids, vec![identifier("urn:ngsi-ld:A:1")]);
        pretty_assertions::assert_eq!(compiled[0].id_pattern, None);
        pretty_assertions::assert_eq!(compiled[0].type_selection, Some(type_iri("A")));
        pretty_assertions::assert_eq!(compiled[1].ids, Vec::<Identifier>::new());
        pretty_assertions::assert_eq!(compiled[1].id_pattern, constraints[1].id_pattern);
        pretty_assertions::assert_eq!(compiled[1].type_selection, TypeSelection::any_of([type_iri("B"), type_iri("C")]));
    }

    /// Clause 6.4.3.2 leaves `type` optional on a GET query, so a selector
    /// without one constrains no type.
    #[tokio::test]
    async fn a_selector_without_a_type_constrains_none() {
        let compiled = expand_entity_constraints(&[EntityConstraint::default()], &EXPANDER)
            .await
            .expect("the selector compiles");
        pretty_assertions::assert_eq!(compiled.len(), 1);
        assert!(!compiled[0].constrains());
    }
}
