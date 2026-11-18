//! Expansion of attribute selection lists through the request's `@context`.

use crate::{
    projection::{
        exclusivity::check_member_exclusivity,
        expanded_projection::ExpandedProjection,
        expanded_projection_term::ExpandedProjectionTerm,
        projection_error::ProjectionError,
        written_projection_term::WrittenProjectionTerm,
    },
    query_error::QueryError,
    term_expander::TermExpander,
};
use iri_rs::IriBuf;
use std::{future::Future, pin::Pin};

/// Expands a list of short attribute names to their full IRIs, for every attribute selection list the API accepts: `attrs` on entity and temporal queries, the CSR property/relationship names, and the `expandValues` / `jsonKeys` extensions.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when a name does not expand through the
/// active `@context`, carrying
/// [`TermExpansionError::NotAnIri`](crate::term_expander::TermExpansionError::NotAnIri)
/// when it expands to something that is not an absolute IRI.
pub async fn expand_attrs<S: AsRef<str> + Sync>(raw: &[S], expander: &impl TermExpander) -> Result<Vec<IriBuf>, QueryError> {
    let mut expanded_iris = Vec::with_capacity(raw.len());
    for term in raw {
        expanded_iris.push(expander.expand_term(term.as_ref()).await?);
    }
    Ok(expanded_iris)
}

/// Expands the `pick` and `omit` lists of one request into its clause 4.21 projection, refusing one whose two lists name the same member once expanded.
///
/// # Errors
///
/// Returns what [`expand_projection_terms`] returns for either list, and
/// [`ProjectionError::MemberPickedAndOmitted`] when `pick` and `omit`
/// name one member.
pub async fn expand_projection(
    pick: &[WrittenProjectionTerm],
    omit: &[WrittenProjectionTerm],
    expander: &impl TermExpander,
) -> Result<ExpandedProjection, QueryError> {
    let pick = expand_projection_terms(pick, expander).await?;
    let omit = expand_projection_terms(omit, expander).await?;
    check_member_exclusivity(&pick, &omit)?;
    Ok(ExpandedProjection { pick, omit })
}

/// Expands a `pick` / `omit` term list (clause 4.21), keeping the keywords `id`, `type` and `scope` as written and recursing into each term's linked-entity sub-projection.
///
/// # Errors
///
/// Returns [`QueryError::TermExpansion`] when an attribute term does not
/// expand, carrying
/// [`TermExpansionError::NotAnIri`](crate::term_expander::TermExpansionError::NotAnIri)
/// when it expands to something that is not an absolute IRI, and
/// [`ProjectionError::KeywordWithLinkedTerm`] when `id`, `type` or `scope`
/// carries a linked-entity sub-projection.
pub async fn expand_projection_terms(terms: &[WrittenProjectionTerm], expander: &impl TermExpander) -> Result<Vec<ExpandedProjectionTerm>, QueryError> {
    expand_projection_terms_inner(terms, expander).await
}

/// The boxed future the recursive walk over `WrittenProjectionTerm::linked` returns; async fns cannot recurse directly because their future type would be infinitely sized.
fn expand_projection_terms_inner<'a, E: TermExpander>(
    terms: &'a [WrittenProjectionTerm],
    expander: &'a E,
) -> Pin<Box<dyn Future<Output = Result<Vec<ExpandedProjectionTerm>, QueryError>> + Send + 'a>> {
    Box::pin(async move {
        let mut expanded_terms = Vec::with_capacity(terms.len());
        for term in terms {
            expanded_terms.push(expand_one_term(term, expander).await?);
        }
        Ok(expanded_terms)
    })
}

/// Expand one written term, recursing into its linked-entity sub-projection when
/// it is an attribute term.
async fn expand_one_term<E: TermExpander>(term: &WrittenProjectionTerm, expander: &E) -> Result<ExpandedProjectionTerm, QueryError> {
    // Clause 4.21: the keywords name structural members, not attributes, so they have no IRI to expand to and no linked entity to project (`LinkedEntityTerm` only ever follows `AttrName`).
    if let Some(keyword) = ExpandedProjectionTerm::keyword(term.name.as_str()) {
        return keyword_term(term, keyword);
    }
    let iri = expander.expand_term(term.name.as_str()).await?;
    let linked = expand_projection_terms_inner(&term.linked, expander).await?;
    Ok(ExpandedProjectionTerm::Attribute { iri, linked })
}

/// Build the expanded keyword term, rejecting a linked-entity sub-projection
/// attached to it.
fn keyword_term(term: &WrittenProjectionTerm, expanded: ExpandedProjectionTerm) -> Result<ExpandedProjectionTerm, QueryError> {
    if term.linked.is_empty() {
        Ok(expanded)
    } else {
        Err(ProjectionError::KeywordWithLinkedTerm {
            keyword: term.name.clone(), // clone: the written projection is borrowed and the refusal outlives it
        }
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        namespace_expander::EXAMPLE_NAMESPACE_TYPED,
        projection::{projection_error::ProjectionError, written_projection_list::WrittenProjectionList},
        term_expander::TermExpansionError,
    };
    use pretty_assertions::assert_eq;
    use std::future::ready;

    #[tokio::test]
    async fn every_attribute_name_is_expanded() {
        let expanded = expand_attrs(&["speed".to_string(), "colour".to_string()], &EXAMPLE_NAMESPACE_TYPED)
            .await
            .expect("expands");
        let rendered: Vec<&str> = expanded.iter().map(IriBuf::as_str).collect();
        assert_eq!(rendered, vec!["https://example.com/speed", "https://example.com/colour"]);
    }

    fn leaf(name: &str) -> WrittenProjectionTerm {
        WrittenProjectionTerm {
            name: name.parse().expect("valid attribute name"),
            linked: Vec::new(),
        }
    }

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    #[tokio::test]
    async fn a_projection_term_of_id_type_or_scope_is_not_expanded() {
        let terms = [leaf("id"), leaf("type"), leaf("scope"), leaf("speed")];
        let expanded = expand_projection_terms(&terms, &EXAMPLE_NAMESPACE_TYPED).await.expect("expands");
        assert_eq!(
            expanded,
            vec![
                ExpandedProjectionTerm::Id,
                ExpandedProjectionTerm::Type,
                ExpandedProjectionTerm::Scope,
                ExpandedProjectionTerm::Attribute {
                    iri: iri("https://example.com/speed"),
                    linked: Vec::new()
                },
            ]
        );
    }

    /// Clause 4.21: `observation{temperature}` selects the `observation` relationship together with the `temperature` attribute of the entity it links to.
    #[tokio::test]
    async fn a_linked_entity_term_expands_recursively() {
        let terms = [WrittenProjectionTerm {
            name: "observation".parse().expect("valid attribute name"),
            linked: vec![leaf("temperature")],
        }];
        let expanded = expand_projection_terms(&terms, &EXAMPLE_NAMESPACE_TYPED).await.expect("expands");
        assert_eq!(
            expanded,
            vec![ExpandedProjectionTerm::Attribute {
                iri: iri("https://example.com/observation"),
                linked: vec![ExpandedProjectionTerm::Attribute {
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
        let terms = [WrittenProjectionTerm {
            name: "observation".parse().expect("valid attribute name"),
            linked: vec![leaf("id"), leaf("type"), leaf("scope")],
        }];
        let expanded = expand_projection_terms(&terms, &EXAMPLE_NAMESPACE_TYPED).await.expect("expands");
        assert_eq!(
            expanded,
            vec![ExpandedProjectionTerm::Attribute {
                iri: iri("https://example.com/observation"),
                linked: vec![ExpandedProjectionTerm::Id, ExpandedProjectionTerm::Type, ExpandedProjectionTerm::Scope],
            }]
        );
    }

    /// Expands every term through one table, the way a request `@context`
    /// maps short names to IRIs; a name it does not hold stays as written.
    struct TableExpander(&'static [(&'static str, &'static str)]);

    impl TermExpander for TableExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            let expanded = self.0.iter().find(|(short, _)| *short == term).map_or(term, |(_, full)| *full);
            ready(Ok(IriBuf::new(expanded.to_owned()).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
            self.expand_term(term)
        }
    }

    const SCHEMA_NAME: TableExpander = TableExpander(&[
        ("name", "https://schema.org/name"),
        ("label", "https://example.com/name"),
        ("owner", "https://example.com/owner"),
        ("email", "https://example.com/email"),
    ]);

    fn list(raw: &str) -> Vec<WrittenProjectionTerm> {
        raw.parse::<WrittenProjectionList>().map(Vec::from).expect("valid projection list")
    }

    async fn projected(pick: &str, omit: &str) -> Result<ExpandedProjection, QueryError> {
        expand_projection(&list(pick), &list(omit), &SCHEMA_NAME).await
    }

    /// Clause 5.5.7: `name` and `https://schema.org/name` are one member under a context mapping the one to the other.
    #[tokio::test]
    async fn a_short_name_against_its_full_iri_is_refused() {
        let error = projected("name", "https://schema.org/name").await.expect_err("one member in both");
        assert!(
            matches!(&error, QueryError::Projection(ProjectionError::MemberPickedAndOmitted { member }) if member.to_string() == "https://schema.org/name"),
            "got {error:?}"
        );
    }

    /// The comparison descends into a linked-entity group at the same level.
    #[tokio::test]
    async fn a_linked_member_spelled_two_ways_is_refused() {
        let error = projected("owner{name,email}", "owner{https://schema.org/name}")
            .await
            .expect_err("one member in both");
        assert!(
            matches!(error, QueryError::Projection(ProjectionError::MemberPickedAndOmitted { .. })),
            "got {error:?}"
        );
    }

    /// Two short names a context maps to different IRIs are different members.
    #[tokio::test]
    async fn two_short_names_expanding_to_different_iris_are_admitted() {
        let projection = projected("name", "label").await.expect("different members");
        assert_eq!(
            projection.pick,
            vec![ExpandedProjectionTerm::Attribute {
                iri: iri("https://schema.org/name"),
                linked: Vec::new()
            }]
        );
        assert_eq!(
            projection.omit,
            vec![ExpandedProjectionTerm::Attribute {
                iri: iri("https://example.com/name"),
                linked: Vec::new()
            }]
        );
    }

    /// Clause 4.21: `LinkedEntityTerm` only ever follows `AttrName`, so `id{a}` is rejected.
    #[tokio::test]
    async fn a_keyword_carrying_a_linked_term_is_rejected() {
        let terms = [WrittenProjectionTerm {
            name: "id".parse().expect("valid attribute name"),
            linked: vec![leaf("a")],
        }];
        let error = expand_projection_terms(&terms, &EXAMPLE_NAMESPACE_TYPED).await.expect_err("must reject");
        assert!(
            matches!(&error, QueryError::Projection(ProjectionError::KeywordWithLinkedTerm { keyword }) if keyword.as_str() == "id"),
            "got {error:?}"
        );
    }
}
