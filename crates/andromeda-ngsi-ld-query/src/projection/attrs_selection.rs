//! Admission and expansion of the deprecated synonym for `pick`: the `attrs`
//! parameter and the `attributes` member of a Subscription's notification.
//!
//! Every `attrs` row, and the `attributes` row of Table 5.2.14.1-1, restricts
//! the member to Attribute names: "A synonym for pick, except that id, type,
//! scope are not allowed". Which spelling a name arrives in does not matter: a
//! context alias of the Scope and its expanded IRI name the same Entity member
//! as the compact term `scope`, so the refusal is decided on the expanded name.

use crate::{
    projection::{pick_synonym::PickSynonym, projection_error::ProjectionError},
    query_error::QueryError,
    term_expander::TermExpander,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties as expanded_properties, keyword};
use iri_rs::IriBuf;

/// Which Entity members the deprecated synonym for `pick` admits beside
/// Attribute names, by the operation that reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttrsAdmission {
    /// `attrs` of Query Entities, Retrieve Entity and the purge selection.
    /// Clause 4.18 keeps the Scope non-reified in an Entity representation, so
    /// it is no Attribute there.
    AttributeNamesOnly,
    /// `attrs` of Query Temporal Evolution of Entities and its Entity Map.
    /// Clause 4.18 reifies the Scope in the temporal representation, where it
    /// is a Property.
    ReifiedScopeAdmitted,
    /// `notification.attributes` of a Subscription (Table 5.2.14.1-1). A
    /// notification carries Entity representations, where clause 4.18 keeps
    /// the Scope non-reified.
    NotificationAttributes,
}

impl AttrsAdmission {
    /// Whether this admission lets the synonym name the Scope.
    const fn admits_scope(self) -> bool {
        match self {
            AttrsAdmission::AttributeNamesOnly | AttrsAdmission::NotificationAttributes => false,
            AttrsAdmission::ReifiedScopeAdmitted => true,
        }
    }

    /// The member the operation reads the synonym from.
    const fn synonym(self) -> PickSynonym {
        match self {
            AttrsAdmission::AttributeNamesOnly | AttrsAdmission::ReifiedScopeAdmitted => PickSynonym::AttrsParameter,
            AttrsAdmission::NotificationAttributes => PickSynonym::NotificationAttributes,
        }
    }

    /// The refusal of `name`, an Entity member this admission does not take.
    fn refusal(self, name: &AttributeName) -> QueryError {
        ProjectionError::ReservedAttrsMember {
            term: name.clone(), // clone: the refusal outlives the borrowed request
            synonym: self.synonym(),
        }
        .into()
    }
}

/// Expand the names of one request's synonym for `pick`, refusing any that
/// name an Entity member the admission does not take.
///
/// The structural members `id` and `type` are refused under every admission,
/// and the Scope only where the admission does not take it. The Scope is also
/// refused when a context term expands to its IRI.
///
/// # Errors
///
/// Returns [`ProjectionError::ReservedAttrsMember`] for a refused member, and
/// [`QueryError::TermExpansion`] when a name does not expand through the active
/// `@context`, carrying
/// [`TermExpansionError::NotAnIri`](crate::term_expander::TermExpansionError::NotAnIri)
/// when it expands to something that is not an absolute IRI.
pub async fn expand_attrs_admitted(raw: &[AttributeName], admission: AttrsAdmission, expander: &impl TermExpander) -> Result<Vec<IriBuf>, QueryError> {
    let mut iris = Vec::with_capacity(raw.len());
    for name in raw {
        if refused_keyword(name, admission) {
            return Err(admission.refusal(name));
        }
        let iri = expander.expand_term(name.as_str()).await?;
        if iri.as_str() == expanded_properties::SCOPE.as_str() && !admission.admits_scope() {
            return Err(admission.refusal(name));
        }
        iris.push(iri);
    }
    Ok(iris)
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
    use crate::{
        namespace_expander::{EXAMPLE_NAMESPACE_TYPED, NamespaceExpander},
        projection::{
            attrs_selection::{AttrsAdmission, expand_attrs_admitted},
            pick_synonym::PickSynonym,
            projection_error::ProjectionError,
        },
        query_error::QueryError,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    /// Expands `sc` to the Scope IRI like the shared expander does `scope`.
    const SCOPE_ALIASES: NamespaceExpander =
        EXAMPLE_NAMESPACE_TYPED.with_core_members(&[("scope", expanded_properties::SCOPE), ("sc", expanded_properties::SCOPE)]);

    fn names(raw: &[&str]) -> Vec<AttributeName> {
        raw.iter().map(|name| name.parse::<AttributeName>().expect("a valid name")).collect()
    }

    /// Whether `result` refuses `member` as written in `synonym`.
    fn refused_in(result: &Result<Vec<IriBuf>, QueryError>, member: &str, synonym: PickSynonym) -> bool {
        matches!(
            result,
            Err(QueryError::Projection(ProjectionError::ReservedAttrsMember { term, synonym: refused_in })) if term.as_str() == member && *refused_in == synonym
        )
    }

    #[tokio::test]
    async fn attribute_names_expand_in_request_order() {
        let expanded = expand_attrs_admitted(
            &names(&["speed", "https://example.org/temperature"]),
            AttrsAdmission::AttributeNamesOnly,
            &SCOPE_ALIASES,
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
        for (admission, synonym) in [
            (AttrsAdmission::AttributeNamesOnly, PickSynonym::AttrsParameter),
            (AttrsAdmission::ReifiedScopeAdmitted, PickSynonym::AttrsParameter),
            (AttrsAdmission::NotificationAttributes, PickSynonym::NotificationAttributes),
        ] {
            for member in ["id", "type"] {
                let refused = expand_attrs_admitted(&names(&["speed", member]), admission, &SCOPE_ALIASES).await;
                assert!(
                    refused_in(&refused, member, synonym),
                    "{member} must be refused in {synonym} under {admission:?}, got {refused:?}"
                );
            }
        }
    }

    #[tokio::test]
    async fn the_compact_scope_is_refused_only_where_the_scope_is_not_an_attribute() {
        let refused = expand_attrs_admitted(&names(&["scope"]), AttrsAdmission::AttributeNamesOnly, &SCOPE_ALIASES).await;
        assert!(refused_in(&refused, "scope", PickSynonym::AttrsParameter), "{refused:?}");

        let admitted = expand_attrs_admitted(&names(&["scope"]), AttrsAdmission::ReifiedScopeAdmitted, &SCOPE_ALIASES).await;
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
            &SCOPE_ALIASES,
        )
        .await;
        assert!(
            refused_in(&refused, "https://uri.etsi.org/ngsi-ld/scope", PickSynonym::AttrsParameter),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_context_alias_of_the_scope_is_refused_as_the_scope_under_attribute_names_only() {
        let refused = expand_attrs_admitted(&names(&["sc"]), AttrsAdmission::AttributeNamesOnly, &SCOPE_ALIASES).await;
        assert!(refused_in(&refused, "sc", PickSynonym::AttrsParameter), "{refused:?}");
    }

    /// Table 5.2.14.1-1: `attributes` is "A synonym for pick, except that id,
    /// type, scope are not allowed", and a notification carries Entity
    /// representations, where clause 4.18 keeps the Scope non-reified.
    #[tokio::test]
    async fn the_notification_attributes_refuse_the_scope_in_every_spelling() {
        for member in ["scope", "sc", "https://uri.etsi.org/ngsi-ld/scope"] {
            let refused = expand_attrs_admitted(&names(&["speed", member]), AttrsAdmission::NotificationAttributes, &SCOPE_ALIASES).await;
            assert!(refused_in(&refused, member, PickSynonym::NotificationAttributes), "{member}: {refused:?}");
        }
    }

    #[test]
    fn a_refusal_names_the_member_the_request_wrote() {
        let refusal = ProjectionError::ReservedAttrsMember {
            term: "scope".parse().expect("a valid name"),
            synonym: PickSynonym::NotificationAttributes,
        };
        assert_eq!(refusal.to_string(), "'scope' is an Entity member and is not allowed in notification.attributes");
    }
}
