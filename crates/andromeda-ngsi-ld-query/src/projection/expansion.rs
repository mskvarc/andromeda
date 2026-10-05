//! Expansion of attribute selection lists through the request's `@context`.

use crate::{
    error::QueryError,
    projection::{grammar::RawProjectionTerm, term::ProjectionTerm},
    term_expander::TermExpander,
};
use iri_rs::IriBuf;
use std::{future::Future, pin::Pin};

/// Expand a list of short attribute names to their full IRIs.
///
/// Used for every attribute selection list the API accepts: `attrs` on entity
/// and temporal queries, the CSR property/relationship names, and the
/// `expandValues` / `jsonKeys` extensions.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] or [`QueryError::TermNotAnIri`] when a
/// name does not expand to an absolute IRI through the active `@context`.
pub async fn expand_attrs<S: AsRef<str> + Sync>(raw: &[S], expander: &impl TermExpander) -> Result<Vec<IriBuf>, QueryError> {
    let mut expanded_iris = Vec::with_capacity(raw.len());
    for term in raw {
        expanded_iris.push(expander.expand_term(term.as_ref()).await?);
    }
    Ok(expanded_iris)
}

/// Expand a `pick` / `omit` term list (clause 4.21), keeping the keywords
/// `id`, `type` and `scope` as written and recursing into each term's clause
/// 4.21 linked-entity sub-projection.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] or [`QueryError::TermNotAnIri`] when an
/// attribute term does not expand, and
/// [`QueryError::ProjectionKeywordWithLinkedTerm`] when `id`, `type` or `scope`
/// carries a linked-entity sub-projection.
pub async fn expand_projection_terms(raw: &[RawProjectionTerm], expander: &impl TermExpander) -> Result<Vec<ProjectionTerm>, QueryError> {
    expand_projection_terms_inner(raw, expander).await
}

/// The boxed future the recursive walk over `RawProjectionTerm::linked` returns.
///
/// Async fns cannot recurse directly (their future type would be infinitely
/// sized), so the recursive step is boxed — the same pattern
/// `EntityQueryService::resolve_linked_inner` uses for the clause 4.5.20 join
/// walk.
fn expand_projection_terms_inner<'a, E: TermExpander>(
    raw: &'a [RawProjectionTerm],
    expander: &'a E,
) -> Pin<Box<dyn Future<Output = Result<Vec<ProjectionTerm>, QueryError>> + Send + 'a>> {
    Box::pin(async move {
        let mut expanded_terms = Vec::with_capacity(raw.len());
        for term in raw {
            expanded_terms.push(expand_one_term(term, expander).await?);
        }
        Ok(expanded_terms)
    })
}

/// Expand one raw term, recursing into its linked-entity sub-projection when
/// it is an attribute term.
async fn expand_one_term<E: TermExpander>(term: &RawProjectionTerm, expander: &E) -> Result<ProjectionTerm, QueryError> {
    // Clause 4.21: the keywords name structural members of the
    // representation, not attributes, so they have no IRI to expand to and no
    // linked entity to project — `LinkedEntityTerm` only ever follows `AttrName`.
    if let Some(keyword) = ProjectionTerm::keyword(term.name.as_str()) {
        return keyword_term(term, keyword);
    }
    let iri = expander.expand_term(term.name.as_str()).await?;
    let linked = expand_projection_terms_inner(&term.linked, expander).await?;
    Ok(ProjectionTerm::Attribute { iri, linked })
}

/// Build the expanded keyword term, rejecting a linked-entity sub-projection
/// attached to it.
fn keyword_term(term: &RawProjectionTerm, expanded: ProjectionTerm) -> Result<ProjectionTerm, QueryError> {
    if term.linked.is_empty() {
        Ok(expanded)
    } else {
        Err(QueryError::ProjectionKeywordWithLinkedTerm {
            keyword: term.name.clone(), // clone: the raw projection is borrowed and the refusal outlives it
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::future::ready;

    struct PrefixExpander;

    impl TermExpander for PrefixExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI")))
        }
    }

    #[tokio::test]
    async fn every_attribute_name_is_expanded() {
        let expanded = expand_attrs(&["speed".to_string(), "colour".to_string()], &PrefixExpander)
            .await
            .expect("expands");
        let rendered: Vec<&str> = expanded.iter().map(IriBuf::as_str).collect();
        assert_eq!(rendered, vec!["https://example.com/speed", "https://example.com/colour"]);
    }

    fn leaf(name: &str) -> RawProjectionTerm {
        RawProjectionTerm {
            name: name.parse().expect("valid attribute name"),
            linked: Vec::new(),
        }
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    #[tokio::test]
    async fn a_projection_term_of_id_type_or_scope_is_not_expanded() {
        // Clause 4.21: `id`, `type` and `scope` name structural members of the
        // representation, not attributes, so they have no IRI to expand to.
        let raw = [leaf("id"), leaf("type"), leaf("scope"), leaf("speed")];
        let expanded = expand_projection_terms(&raw, &PrefixExpander).await.expect("expands");
        assert_eq!(
            expanded,
            vec![
                ProjectionTerm::Id,
                ProjectionTerm::Type,
                ProjectionTerm::Scope,
                ProjectionTerm::Attribute {
                    iri: iri("https://example.com/speed"),
                    linked: Vec::new()
                },
            ]
        );
    }

    /// Clause 4.21: `observation{temperature}` selects the `observation`
    /// relationship together with the `temperature` attribute of the entity
    /// it links to.
    #[tokio::test]
    async fn a_linked_entity_term_expands_recursively() {
        let raw = [RawProjectionTerm {
            name: "observation".parse().expect("valid attribute name"),
            linked: vec![leaf("temperature")],
        }];
        let expanded = expand_projection_terms(&raw, &PrefixExpander).await.expect("expands");
        assert_eq!(
            expanded,
            vec![ProjectionTerm::Attribute {
                iri: iri("https://example.com/observation"),
                linked: vec![ProjectionTerm::Attribute {
                    iri: iri("https://example.com/temperature"),
                    linked: Vec::new()
                }],
            }]
        );
    }

    /// Nested keyword terms (e.g. `observation{id,type}`) expand to the
    /// clause 4.21 keyword variants, not to attribute terms.
    #[tokio::test]
    async fn nested_keywords_expand_to_the_keyword_variants() {
        let raw = [RawProjectionTerm {
            name: "observation".parse().expect("valid attribute name"),
            linked: vec![leaf("id"), leaf("type"), leaf("scope")],
        }];
        let expanded = expand_projection_terms(&raw, &PrefixExpander).await.expect("expands");
        assert_eq!(
            expanded,
            vec![ProjectionTerm::Attribute {
                iri: iri("https://example.com/observation"),
                linked: vec![ProjectionTerm::Id, ProjectionTerm::Type, ProjectionTerm::Scope],
            }]
        );
    }

    /// Clause 4.21: `LinkedEntityTerm` only ever follows `AttrName`, so a
    /// keyword carrying `{...}` (e.g. `id{a}`) is rejected.
    #[tokio::test]
    async fn a_keyword_carrying_a_linked_term_is_rejected() {
        let raw = [RawProjectionTerm {
            name: "id".parse().expect("valid attribute name"),
            linked: vec![leaf("a")],
        }];
        let error = expand_projection_terms(&raw, &PrefixExpander).await.expect_err("must reject");
        assert!(
            matches!(&error, QueryError::ProjectionKeywordWithLinkedTerm { keyword } if keyword.as_str() == "id"),
            "got {error:?}"
        );
    }
}
