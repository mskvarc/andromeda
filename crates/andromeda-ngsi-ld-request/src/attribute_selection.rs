//! Validation for the deprecated `attrs` selection parameter.

use crate::error::ParameterError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// The `scope` Entity member as a request spells it in `attrs`, `pick` and
/// `omit`.
const SCOPE_MEMBER: &str = "scope";

/// Which Entity members the deprecated `attrs` synonym admits beside Attribute
/// names.
///
/// Every `attrs` row in the specification restricts the parameter the same way:
/// "Each String is an Attribute (Property or Relationship) name." Table
/// 6.4.3.2-1 says it of Query Entities, Table 6.5.3.1-1 of Retrieve Entity,
/// Table 6.8.3.2-1 of Query Context Source Registrations, Table 6.18.3.2-1 of
/// Query Temporal Evolution of Entities and Table 6.19.3.1-1 of Retrieve
/// Temporal Evolution of an Entity. `pick` is the parameter that is not so
/// restricted -- "Each String is an Entity member ("id", "type", "scope" or an
/// Attribute name)" -- which is why the two differ at all.
///
/// The restricting sentence is therefore what decides, and not the sentence
/// above it: the retrieval rows add "except that id, type, scope are not
/// allowed" because they call `attrs` "a synonym for the pick parameter" with
/// nothing else said, while the query rows call it "a synonym for a combination
/// of the pick and q parameters" and restrict in the next breath. An omission
/// cannot grant what the row it sits in withholds.
///
/// What the restriction admits is then a question about `scope` rather than
/// about `attrs`, and clause 4.18 answers it in one sentence: "The Scope is
/// represented as a special scope Property that is non-reified in the
/// normalized Entity representation and reified in the temporal representation
/// of an Entity." In an Entity representation `scope` is an Entity member and
/// no Attribute at all, so `attrs=scope` names nothing the row admits. In a
/// temporal representation it is a Property, so the row admits it -- and clause
/// 4.18 points at annex C clause C.5.16 for the Temporal Evolution of a Scope,
/// which is a `GET /temporal/entities` carrying exactly `attrs=speed,scope`.
/// That annex is informative (clause C.1) and settles nothing on its own; it
/// corroborates a reading the normative row already reaches.
///
/// Table 6.19.3.1-1 then takes it back for the temporal retrieval alone, in so
/// many words, and an explicit prohibition beats an inference from the
/// representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AttrsAdmission {
    /// Attribute names alone.
    ///
    /// Query Entities and Retrieve Entity answer in an Entity representation
    /// where clause 4.18 leaves the Scope non-reified, so `scope` is no
    /// Attribute name there; Retrieve Temporal Evolution of an Entity answers in
    /// a representation where it is one, and Table 6.19.3.1-1 excludes it
    /// anyway.
    AttributeNamesOnly,
    /// The `scope` Property beside the Attribute names.
    ///
    /// Query Temporal Evolution of Entities answers in the representation clause
    /// 4.18 reifies the Scope in, so `scope` is an Attribute name there and
    /// Table 6.18.3.2-1 excludes nothing.
    ReifiedScopeAdmitted,
}

impl AttrsAdmission {
    /// Whether this admission lets `attrs` name the Entity member `name`.
    fn admits(self, name: &AttributeName) -> bool {
        match self {
            AttrsAdmission::AttributeNamesOnly => false,
            AttrsAdmission::ReifiedScopeAdmitted => name.as_str() == SCOPE_MEMBER,
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
