//! Validation for the deprecated `attrs` selection parameter.

use crate::error::ParameterError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

/// Which Entity members the deprecated `attrs` synonym admits beside Attribute names.
///
/// Every `attrs` row restricts the parameter to "an Attribute (Property or Relationship) name", while `pick` admits Entity members. Clause 4.18 keeps the Scope non-reified in an Entity representation, so `scope` is no Attribute there, and reifies it in the temporal representation, where it is a Property. Table 6.19.3.1-1 nevertheless excludes it for the temporal retrieval alone, and an explicit prohibition beats an inference from the representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttrsAdmission {
    /// Query Entities, Retrieve Entity and Retrieve Temporal Evolution of an Entity.
    AttributeNamesOnly,
    /// Query Temporal Evolution of Entities.
    ReifiedScopeAdmitted,
}

impl AttrsAdmission {
    /// Whether this admission lets `attrs` name the Entity member `name`.
    fn admits(self, name: &AttributeName) -> bool {
        match self {
            AttrsAdmission::AttributeNamesOnly => false,
            AttrsAdmission::ReifiedScopeAdmitted => name.as_str() == compact_properties::SCOPE,
        }
    }
}

/// Validate that `attrs` names only what the operation's own table admits.
pub(crate) fn validate_attrs(attrs: Option<&[AttributeName]>, admission: AttrsAdmission) -> Result<(), ParameterError> {
    let reserved = attrs
        .into_iter()
        .flatten()
        .filter(|attribute| matches!(attribute.as_str(), "id" | "type" | compact_properties::SCOPE))
        .find(|attribute| !admission.admits(attribute));
    match reserved {
        Some(attribute) => Err(ParameterError::AttrsReservedMember(attribute.clone())),
        None => Ok(()),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute_selection::{AttrsAdmission, validate_attrs},
        error::ParameterError,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    fn attrs(raw: &[&str]) -> Vec<AttributeName> {
        raw.iter().map(|name| name.parse::<AttributeName>().expect("a valid name")).collect()
    }

    /// Every `attrs` row restricts the parameter to "an Attribute (Property or
    /// Relationship) name", and clause 4.18 leaves the Scope non-reified in an
    /// Entity representation, so none of the three structural members is one:
    /// Tables 6.4.3.2-1, 6.5.3.1-1 and 6.19.3.1-1 all refuse them.
    #[test]
    fn an_entity_representation_admits_no_entity_member_in_attrs() {
        for member in ["id", "type", "scope"] {
            let selection = attrs(&["speed", member]);
            assert!(
                matches!(validate_attrs(Some(&selection), AttrsAdmission::AttributeNamesOnly), Err(ParameterError::AttrsReservedMember(named)) if named.as_str() == member),
                "{member} must be refused"
            );
        }
    }

    /// Clause 4.18 reifies the Scope in the temporal representation, so `scope`
    /// satisfies Table 6.18.3.2-1's "Each String is an Attribute (Property or
    /// Relationship) name" on the temporal query, which is the request annex C
    /// clause C.5.16 prints.
    #[test]
    fn the_temporal_query_admits_the_reified_scope_property_in_attrs() {
        let selection = attrs(&["speed", "scope"]);
        assert!(validate_attrs(Some(&selection), AttrsAdmission::ReifiedScopeAdmitted).is_ok());
    }

    /// `scope` is the one member clause 4.18 reifies. `id` and `type` stay
    /// Entity members in every representation, so no `attrs` row admits them.
    #[test]
    fn the_wider_admission_still_refuses_the_identity_members() {
        for member in ["id", "type"] {
            let selection = attrs(&["speed", member]);
            assert!(
                matches!(validate_attrs(Some(&selection), AttrsAdmission::ReifiedScopeAdmitted), Err(ParameterError::AttrsReservedMember(named)) if named.as_str() == member),
                "{member} must be refused"
            );
        }
    }

    #[test]
    fn genuine_attribute_names_are_accepted_by_either_admission() {
        let selection = attrs(&["speed"]);
        assert!(validate_attrs(Some(&selection), AttrsAdmission::AttributeNamesOnly).is_ok());
        assert!(validate_attrs(Some(&selection), AttrsAdmission::ReifiedScopeAdmitted).is_ok());
    }

    /// An absent or empty `attrs` names nothing to refuse.
    #[test]
    fn an_absent_or_empty_attrs_is_admitted_by_either_admission() {
        for admission in [AttrsAdmission::AttributeNamesOnly, AttrsAdmission::ReifiedScopeAdmitted] {
            assert!(validate_attrs(None, admission).is_ok());
            assert!(validate_attrs(Some(&[]), admission).is_ok());
        }
    }
}
