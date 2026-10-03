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
//! What is checked here is the shape a client wrote, before any expansion:
//! JSON-LD expansion turns a single object and a one-item array into the same
//! node, so "contained by an Array" can only be read on the written body.
//! Whether the one Attribute is the one the request targets needs term
//! expansion and is decided where the target Attribute is resolved.

use crate::fragment_entity_id::{FragmentEntityIdError, admit_fragment_entity_id};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, reference::NgsiLdReferenceError};
use serde_json::Value;
use thiserror::Error;

/// The members of a Fragment that are not Attributes: the JSON-LD `@context`,
/// and the `id` and `type` clause 5.4 allows, in either spelling.
const NON_ATTRIBUTE_MEMBERS: [&str; 5] = ["@context", "id", "type", "@id", "@type"];

/// Why a body is not the `EntityTemporal` Fragment clause 5.6.14.3 requires.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TemporalInstanceFragmentError {
    /// The body is not a JSON object (clause 5.4).
    #[error("the body must be an EntityTemporal Fragment, a JSON object naming the attribute")]
    NotAnObject,
    /// The Fragment names no Attribute.
    #[error("the EntityTemporal Fragment names no attribute")]
    NoAttribute,
    /// The Fragment names more than one Attribute.
    #[error("the EntityTemporal Fragment names more than one attribute; a modification carries exactly one")]
    SeveralAttributes,
    /// The Attribute member is not an attribute name.
    #[error("the EntityTemporal Fragment member is not an attribute name: {source}")]
    AttributeName {
        /// The grammar violation.
        #[source]
        source: NgsiLdReferenceError,
    },
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
    /// The one item is not an Attribute instance object.
    #[error("the instance of '{attribute}' must be a JSON object")]
    InstanceNotAnObject {
        /// The Attribute as written.
        attribute: AttributeName,
    },
    /// The Fragment's `id` does not name the Entity the request targets
    /// (clause 5.4).
    #[error(transparent)]
    EntityId(#[from] FragmentEntityIdError),
}

/// Check that `body` is the clause 5.6.14.3 `EntityTemporal` Fragment for a
/// modification of `target`: a JSON object naming exactly one Attribute, whose
/// one instance is contained by an Array of exactly one item, and whose `id`,
/// when present, is `target`.
///
/// # Errors
///
/// Returns the [`TemporalInstanceFragmentError`] naming the first way the body
/// departs from that shape.
pub fn admit_temporal_instance_fragment(body: &Value, target: &Identifier) -> Result<(), TemporalInstanceFragmentError> {
    let Value::Object(members) = body else {
        return Err(TemporalInstanceFragmentError::NotAnObject);
    };
    admit_fragment_entity_id(members, target)?;
    let mut attributes = members.iter().filter(|(name, _)| !NON_ATTRIBUTE_MEMBERS.contains(&name.as_str()));
    let (name, value) = match (attributes.next(), attributes.next()) {
        (Some(only), None) => only,
        (None, None | Some(_)) => return Err(TemporalInstanceFragmentError::NoAttribute),
        (Some(_), Some(_)) => return Err(TemporalInstanceFragmentError::SeveralAttributes),
    };
    let attribute = name
        .parse::<AttributeName>()
        .map_err(|source| TemporalInstanceFragmentError::AttributeName { source })?;
    let Value::Array(instances) = value else {
        return Err(TemporalInstanceFragmentError::NotContainedByAnArray { attribute });
    };
    let [instance] = instances.as_slice() else {
        return Err(TemporalInstanceFragmentError::InstanceCount {
            attribute,
            count: instances.len(),
        });
    };
    if !instance.is_object() {
        return Err(TemporalInstanceFragmentError::InstanceNotAnObject { attribute });
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
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn target() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid identifier")
    }

    fn instance() -> Value {
        json!({"type": "Property", "value": 129, "observedAt": "2020-09-01T12:03:00Z"})
    }

    fn refusal(body: &Value) -> TemporalInstanceFragmentError {
        admit_temporal_instance_fragment(body, &target()).expect_err("the body is refused")
    }

    /// Clause 5.6.14.3: one Attribute holding an Array of exactly one item.
    #[test]
    fn the_clause_5_6_14_3_fragment_is_admitted() {
        assert_eq!(admit_temporal_instance_fragment(&json!({"speed": [instance()]}), &target()), Ok(()));
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
        assert_eq!(admit_temporal_instance_fragment(&body, &target()), Ok(()));
    }

    /// A bare instance names no Attribute: its members are the instance's own.
    #[test]
    fn a_bare_instance_is_not_a_fragment() {
        assert!(matches!(refusal(&instance()), TemporalInstanceFragmentError::SeveralAttributes));
    }

    #[test]
    fn a_bare_array_is_not_a_fragment() {
        assert_eq!(refusal(&json!([instance()])), TemporalInstanceFragmentError::NotAnObject);
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
        assert!(matches!(
            refusal(&json!({"speed": []})),
            TemporalInstanceFragmentError::InstanceCount { count: 0, .. }
        ));
    }

    #[test]
    fn an_item_that_is_not_an_instance_object_is_refused() {
        assert!(matches!(
            refusal(&json!({"speed": [129]})),
            TemporalInstanceFragmentError::InstanceNotAnObject { .. }
        ));
    }

    /// Clause 5.4: the `id` "shall be equal to the id of the target".
    #[test]
    fn an_id_naming_another_entity_is_refused() {
        assert!(matches!(
            refusal(&json!({"id": "urn:ngsi-ld:Vehicle:2", "speed": [instance()]})),
            TemporalInstanceFragmentError::EntityId(FragmentEntityIdError::OtherEntity { .. })
        ));
        assert_eq!(
            refusal(&json!({"id": 7, "speed": [instance()]})),
            TemporalInstanceFragmentError::EntityId(FragmentEntityIdError::NotAString)
        );
    }
}
