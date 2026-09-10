//! Validation for the deprecated `attrs` selection parameter.

use crate::error::ParameterError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// The `scope` Entity member as a request spells it in `attrs`, `pick` and
/// `omit`.
const SCOPE_MEMBER: &str = "scope";

/// Which Entity members the deprecated `attrs` synonym admits beside Attribute
/// names.
///
/// The specification does not give one answer for every operation. Tables
/// 6.5.3.1-1 and 6.19.3.1-1 make `attrs` on a retrieval "a synonym for the pick
/// parameter, except that id, type, scope are not allowed", while Tables
/// 6.4.3.2-1 and 6.18.3.2-1 make it on a query "a synonym for a combination of
/// the pick and q parameters" with no such exception — and `pick` names Entity
/// members: "id", "type", "scope" or an Attribute name. Annex C, clause C.5.16
/// is the specification writing such a query itself, with
/// `attrs=speed,scope` against the temporal query endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttrsAdmission {
    /// Attribute names alone: the retrieval reading, where `id`, `type` and
    /// `scope` are named as not allowed.
    AttributeNamesOnly,
    /// The `scope` Entity member beside the Attribute names, which the temporal
    /// query admits because clause 4.18 reifies the Scope in the temporal
    /// representation and `pick` names it.
    ScopeAdmitted,
}

impl AttrsAdmission {
    /// Whether this admission lets `attrs` name the Entity member `name`.
    fn admits(self, name: &AttributeName) -> bool {
        match self {
            AttrsAdmission::AttributeNamesOnly => false,
            AttrsAdmission::ScopeAdmitted => name.as_str() == SCOPE_MEMBER,
        }
    }
}

/// Validate that `attrs` names only what the operation's own table admits.
pub(crate) fn validate_attrs(attrs: Option<&[AttributeName]>, admission: AttrsAdmission) -> Result<(), ParameterError> {
    let reserved = attrs
        .into_iter()
        .flatten()
        .filter(|attribute| matches!(attribute.as_str(), "id" | "type" | SCOPE_MEMBER))
        .find(|attribute| !admission.admits(attribute));
    match reserved {
        Some(attribute) => Err(ParameterError::AttrsReservedMember(attribute.clone())),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_selection::{AttrsAdmission, validate_attrs},
        error::ParameterError,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    fn attrs(raw: &[&str]) -> Vec<AttributeName> {
        raw.iter().map(|name| name.parse::<AttributeName>().expect("a valid name")).collect()
    }

    /// Tables 6.5.3.1-1 and 6.19.3.1-1: on a retrieval `attrs` is "a synonym
    /// for the pick parameter, except that id, type, scope are not allowed".
    #[test]
    fn a_retrieval_admits_no_entity_member_in_attrs() {
        for member in ["id", "type", "scope"] {
            let selection = attrs(&["speed", member]);
            assert!(
                matches!(validate_attrs(Some(&selection), AttrsAdmission::AttributeNamesOnly), Err(ParameterError::AttrsReservedMember(named)) if named.as_str() == member),
                "{member} must be refused"
            );
        }
    }

    /// Table 6.18.3.2-1 makes `attrs` on the temporal query a synonym for pick
    /// and q, and annex C clause C.5.16 queries with `attrs=speed,scope`.
    #[test]
    fn the_temporal_query_admits_the_scope_member_in_attrs() {
        let selection = attrs(&["speed", "scope"]);
        assert!(validate_attrs(Some(&selection), AttrsAdmission::ScopeAdmitted).is_ok());
    }

    /// `scope` is the one Entity member the wider reading admits: `id` and
    /// `type` name no Attribute instance a temporal query could select.
    #[test]
    fn the_wider_admission_still_refuses_the_identity_members() {
        for member in ["id", "type"] {
            let selection = attrs(&["speed", member]);
            assert!(
                matches!(validate_attrs(Some(&selection), AttrsAdmission::ScopeAdmitted), Err(ParameterError::AttrsReservedMember(named)) if named.as_str() == member),
                "{member} must be refused"
            );
        }
    }

    #[test]
    fn genuine_attribute_names_are_accepted_by_either_admission() {
        let selection = attrs(&["speed"]);
        assert!(validate_attrs(Some(&selection), AttrsAdmission::AttributeNamesOnly).is_ok());
        assert!(validate_attrs(Some(&selection), AttrsAdmission::ScopeAdmitted).is_ok());
    }
}
