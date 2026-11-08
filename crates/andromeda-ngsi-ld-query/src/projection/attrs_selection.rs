//! Admission and expansion of the deprecated `attrs` parameter.
//!
//! Every `attrs` row restricts the parameter to "an Attribute (Property or
//! Relationship) name". Which spelling a name arrives in does not matter: a
//! context alias of the Scope and its expanded IRI name the same Entity member
//! as the compact term `scope`, so the refusal is decided on the expanded name.

use crate::{error::QueryError, term_expander::TermExpander};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties as expanded_properties, keyword};
use iri_rs::IriBuf;

/// Which Entity members `attrs` admits beside Attribute names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttrsAdmission {
    /// Query Entities, Retrieve Entity and the purge selection. Clause 4.18
    /// keeps the Scope non-reified in an Entity representation, so it is no
    /// Attribute there.
    AttributeNamesOnly,
    /// Query Temporal Evolution of Entities and its Entity Map. Clause 4.18
    /// reifies the Scope in the temporal representation, where it is a
    /// Property.
    ReifiedScopeAdmitted,
}

impl AttrsAdmission {
    /// Whether this admission lets `attrs` name the Scope.
    const fn admits_scope(self) -> bool {
        match self {
            AttrsAdmission::AttributeNamesOnly => false,
            AttrsAdmission::ReifiedScopeAdmitted => true,
        }
    }
}

/// Expand the `attrs` names of one request, refusing any that name an Entity
/// member the admission does not take.
///
/// The structural members `id` and `type` are refused under every admission,
/// and the Scope only where the admission does not take it. The Scope is also
/// refused when a context term expands to its IRI.
///
/// # Errors
///
/// Returns [`QueryError::AttrsReservedMember`] for a refused member, and
/// [`QueryError::TermExpansion`] or [`QueryError::TermNotAnIri`] when a name
/// does not expand to an absolute IRI through the active `@context`.
pub async fn expand_attrs_admitted(raw: &[AttributeName], admission: AttrsAdmission, expander: &impl TermExpander) -> Result<Vec<IriBuf>, QueryError> {
    let mut expanded = Vec::with_capacity(raw.len());
    for name in raw {
        if refused_keyword(name, admission) {
            return Err(QueryError::AttrsReservedMember { term: name.clone() }); // clone: the refusal outlives the borrowed request
        }
        let iri = expander.expand_term(name.as_str()).await?;
        if iri.as_str() == expanded_properties::SCOPE.as_str() && !admission.admits_scope() {
            return Err(QueryError::AttrsReservedMember { term: name.clone() }); // clone: the refusal outlives the borrowed request
        }
        expanded.push(iri);
    }
    Ok(expanded)
}

/// Whether `name` spells a structural member that `admission` refuses before
/// any expansion.
fn refused_keyword(name: &AttributeName, admission: AttrsAdmission) -> bool {
    match name.as_str() {
        keyword::ID_ALIAS | keyword::TYPE_ALIAS => true,
        compact_properties::SCOPE => !admission.admits_scope(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{AttrsAdmission, expand_attrs_admitted};
    use crate::{error::QueryError, term_expander::TermExpander};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::future::ready;

    /// Expands `sc` to the Scope IRI, passes absolute IRIs through and prefixes
    /// every other term with `https://example.com/`.
    struct ScopeAliasExpander;

    impl TermExpander for ScopeAliasExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            let iri = match term {
                "scope" | "sc" => expanded_properties::SCOPE.as_str().to_owned(),
                _ if term.starts_with("https://") => term.to_owned(),
                _ => format!("https://example.com/{term}"),
            };
            ready(Ok(IriBuf::new(iri).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI")))
        }
    }

    fn names(raw: &[&str]) -> Vec<AttributeName> {
        raw.iter().map(|name| name.parse::<AttributeName>().expect("a valid name")).collect()
    }

    #[tokio::test]
    async fn attribute_names_expand_in_request_order() {
        let expanded = expand_attrs_admitted(
            &names(&["speed", "https://example.org/temperature"]),
            AttrsAdmission::AttributeNamesOnly,
            &ScopeAliasExpander,
        )
        .await;
        assert_eq!(
            expanded.expect("attribute names expand"),
            vec![
                IriBuf::new("https://example.com/speed".to_owned()).expect("valid IRI"),
                IriBuf::new("https://example.org/temperature".to_owned()).expect("valid IRI"),
            ]
        );
    }

    #[tokio::test]
    async fn the_compact_identity_members_are_refused_under_every_admission() {
        for admission in [AttrsAdmission::AttributeNamesOnly, AttrsAdmission::ReifiedScopeAdmitted] {
            for member in ["id", "type"] {
                let refused = expand_attrs_admitted(&names(&["speed", member]), admission, &ScopeAliasExpander).await;
                assert!(
                    matches!(refused, Err(QueryError::AttrsReservedMember { ref term }) if term.as_str() == member),
                    "{member} must be refused under {admission:?}, got {refused:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn the_compact_scope_is_refused_only_where_the_scope_is_not_an_attribute() {
        let refused = expand_attrs_admitted(&names(&["scope"]), AttrsAdmission::AttributeNamesOnly, &ScopeAliasExpander).await;
        assert!(
            matches!(refused, Err(QueryError::AttrsReservedMember { ref term }) if term.as_str() == "scope"),
            "{refused:?}"
        );

        let admitted = expand_attrs_admitted(&names(&["scope"]), AttrsAdmission::ReifiedScopeAdmitted, &ScopeAliasExpander).await;
        assert_eq!(
            admitted.expect("the reified scope is an attribute"),
            vec![IriBuf::new(expanded_properties::SCOPE.as_str().to_owned()).expect("valid IRI")]
        );
    }

    #[tokio::test]
    async fn the_expanded_scope_iri_is_refused_as_the_scope_under_attribute_names_only() {
        let refused = expand_attrs_admitted(
            &names(&["https://uri.etsi.org/ngsi-ld/scope"]),
            AttrsAdmission::AttributeNamesOnly,
            &ScopeAliasExpander,
        )
        .await;
        assert!(
            matches!(refused, Err(QueryError::AttrsReservedMember { ref term }) if term.as_str() == "https://uri.etsi.org/ngsi-ld/scope"),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_context_alias_of_the_scope_is_refused_as_the_scope_under_attribute_names_only() {
        let refused = expand_attrs_admitted(&names(&["sc"]), AttrsAdmission::AttributeNamesOnly, &ScopeAliasExpander).await;
        assert!(
            matches!(refused, Err(QueryError::AttrsReservedMember { ref term }) if term.as_str() == "sc"),
            "{refused:?}"
        );
    }
}
