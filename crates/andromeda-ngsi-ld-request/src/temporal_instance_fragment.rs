//! The request body of Modify Attribute instance in Temporal Evolution of an
//! Entity (clause 5.6.14).
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.6.14.3 takes "A JSON-LD document
//! representing an NGSI-LD Fragment of `EntityTemporal`, including only the new
//! Attribute instance, contained by an Array of exactly one item", and Table
//! 6.22.3.1-1 types the body as an `EntityTemporal Fragment`. Clause 5.4 makes
//! a Fragment "a JSON-LD Object" whose members follow "the same data
//! representation and nesting structure" as the target, so the body names the
//! Attribute and holds its instance array: `{"speed": [{instance}]}`, the shape
//! clause 5.6.12.3 uses for Add Attributes, restricted to one Attribute and one
//! instance. Clause 5.4 lets a Fragment carry `id` and `type`, and an `id`
//! "shall be equal to the id of the target (mutated) NGSI-LD element".
//!
//! What is checked here is the shape a client wrote, read from the decoded
//! Fragment: JSON-LD expansion turns a single object and a one-item array into
//! the same node, so "contained by an Array" is read from what the decode kept
//! of the written container. Whether the one Attribute is the one the request
//! targets needs term expansion and is decided where the target Attribute is
//! resolved.

use crate::fragment_entity_id::{FragmentEntityIdError, admit_fragment_entity_id};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::temporal::written_temporal_entity::WrittenTemporalEntity;
use thiserror::Error;

/// Why a body is not the `EntityTemporal` Fragment clause 5.6.14.3 requires.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TemporalInstanceFragmentError {
    /// The Fragment carries a Scope, which is no Attribute instance.
    #[error("the EntityTemporal Fragment carries 'scope'; it includes only the new attribute instance")]
    ScopeMember,
    /// The Fragment carries an `expiresAt`, which is no Attribute instance.
    #[error("the EntityTemporal Fragment carries 'expiresAt'; it includes only the new attribute instance")]
    ExpiresAtMember,
    /// The Fragment names no Attribute.
    #[error("the EntityTemporal Fragment names no attribute")]
    NoAttribute,
    /// The Fragment names more than one Attribute.
    #[error("the EntityTemporal Fragment names more than one attribute; a modification carries exactly one")]
    SeveralAttributes,
    /// The Attribute's instances are not contained by an Array.
    #[error("the instance of '{attribute}' must be contained by an array of exactly one item")]
    NotContainedByAnArray {
        /// The Attribute as written.
        attribute: AttributeName,
    },
    /// The Array does not hold exactly one instance.
    #[error("'{attribute}' carries {count} instances; a modification carries exactly one")]
    InstanceCount {
        /// The Attribute as written.
        attribute: AttributeName,
        /// How many items the Array holds.
        count: usize,
    },
    /// The Fragment's `id` does not name the Entity the request targets
    /// (clause 5.4).
    #[error(transparent)]
    EntityId(#[from] FragmentEntityIdError),
}

/// Check that `body` is the clause 5.6.14.3 `EntityTemporal` Fragment for a
/// modification of `target`: naming exactly one Attribute, whose one instance
/// is contained by an Array of exactly one item, and whose `id`, when present,
/// is `target`.
///
/// # Errors
///
/// Returns the [`TemporalInstanceFragmentError`] naming the first way the body
/// departs from that shape.
pub fn admit_temporal_instance_fragment(body: &WrittenTemporalEntity, target: &Identifier) -> Result<(), TemporalInstanceFragmentError> {
    admit_fragment_entity_id(body.id.as_ref(), target)?;
    // "including only the new Attribute instance": the entity-level members
    // other than the `id` and `type` clause 5.4 admits are not that instance.
    if body.scope.is_some() {
        return Err(TemporalInstanceFragmentError::ScopeMember);
    }
    if body.expires_at.is_some() {
        return Err(TemporalInstanceFragmentError::ExpiresAtMember);
    }
    let mut attributes = body.attrs.iter();
    let (attribute, instances) = match (attributes.next(), attributes.next()) {
        (Some(only), None) => only,
        (None, None | Some(_)) => return Err(TemporalInstanceFragmentError::NoAttribute),
        (Some(_), Some(_)) => return Err(TemporalInstanceFragmentError::SeveralAttributes),
    };
    if body.unarrayed.contains(attribute) {
        // clone: the error owns the attribute it names
        return Err(TemporalInstanceFragmentError::NotContainedByAnArray { attribute: attribute.clone() });
    }
    if instances.len() != 1 {
        return Err(TemporalInstanceFragmentError::InstanceCount {
            attribute: attribute.clone(), // clone: the error owns the attribute it names
            count: instances.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        fragment_entity_id::FragmentEntityIdError,
        temporal_instance_fragment::{TemporalInstanceFragmentError, admit_temporal_instance_fragment},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{request_body::RequestBody, temporal::written_temporal_entity::WrittenTemporalEntity};
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn target() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid identifier")
    }

    fn instance() -> Value {
        json!({"type": "Property", "value": 129, "observedAt": "2020-09-01T12:03:00Z"})
    }

    fn fragment(body: &Value) -> WrittenTemporalEntity {
        WrittenTemporalEntity::from_request_text(body.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a decodable Fragment")
    }

    fn refusal(body: &Value) -> TemporalInstanceFragmentError {
        admit_temporal_instance_fragment(&fragment(body), &target()).expect_err("the body is refused")
    }

    /// Clause 5.6.14.3: one Attribute holding an Array of exactly one item.
    #[test]
    fn the_clause_5_6_14_3_fragment_is_admitted() {
        assert_eq!(admit_temporal_instance_fragment(&fragment(&json!({"speed": [instance()]})), &target()), Ok(()));
    }

    /// Clause 5.4 lets a Fragment carry the `id` of its target, its `type` and
    /// the JSON-LD `@context`.
    #[test]
    fn a_fragment_carrying_its_target_id_type_and_context_is_admitted() {
        let body = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": [instance()],
            "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
        });
        assert_eq!(admit_temporal_instance_fragment(&fragment(&body), &target()), Ok(()));
    }

    #[test]
    fn an_empty_object_names_no_attribute() {
        assert_eq!(refusal(&json!({})), TemporalInstanceFragmentError::NoAttribute);
    }

    #[test]
    fn two_attributes_are_refused() {
        assert_eq!(
            refusal(&json!({"speed": [instance()], "fuel": [instance()]})),
            TemporalInstanceFragmentError::SeveralAttributes
        );
    }

    /// "contained by an Array": a single object is not the stated shape, although
    /// JSON-LD expansion would read it the same way.
    #[test]
    fn an_instance_not_contained_by_an_array_is_refused() {
        assert!(matches!(
            refusal(&json!({"speed": instance()})),
            TemporalInstanceFragmentError::NotContainedByAnArray { .. }
        ));
    }

    /// "an Array of exactly one item".
    #[test]
    fn an_array_of_other_than_one_item_is_refused() {
        assert!(matches!(
            refusal(&json!({"speed": [instance(), instance()]})),
            TemporalInstanceFragmentError::InstanceCount { count: 2, .. }
        ));
    }

    /// A bare instance names no Attribute: its members are the instance's own,
    /// which a Fragment reads as several attributes.
    #[test]
    fn a_bare_instance_is_not_a_fragment() {
        assert_eq!(refusal(&instance()), TemporalInstanceFragmentError::SeveralAttributes);
    }

    /// Clause 5.6.14.3: "including only the new Attribute instance", so a Scope,
    /// reified or as paths, is refused beside it.
    #[test]
    fn a_scope_beside_the_instance_is_refused() {
        for scope in [json!("/Madrid"), json!([{"type": "Property", "value": "/Madrid"}])] {
            assert_eq!(
                refusal(&json!({"scope": scope, "speed": [instance()]})),
                TemporalInstanceFragmentError::ScopeMember
            );
        }
    }

    #[test]
    fn an_expiry_beside_the_instance_is_refused() {
        assert_eq!(
            refusal(&json!({"expiresAt": "2030-01-01T00:00:00Z", "speed": [instance()]})),
            TemporalInstanceFragmentError::ExpiresAtMember
        );
    }

    /// Clause 5.4: the `id` "shall be equal to the id of the target".
    #[test]
    fn an_id_naming_another_entity_is_refused() {
        assert!(matches!(
            refusal(&json!({"id": "urn:ngsi-ld:Vehicle:2", "speed": [instance()]})),
            TemporalInstanceFragmentError::EntityId(FragmentEntityIdError { .. })
        ));
    }
}
