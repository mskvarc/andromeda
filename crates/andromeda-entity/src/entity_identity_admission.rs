//! Screening the identity members of an expanded entity payload before the
//! write it belongs to does anything else (clauses 5.5.4 and 5.6.1.4).
//!
//! An `@id` or `@type` that JSON-LD expansion produced but that is not an
//! identifier or a type name -- one a user `@context` expanded past the
//! identifier bound, say -- is invalid input and a `BadRequestData`. Reading it
//! later, when the payload is stored or indexed, would find the entity with no
//! identifier or with fewer types than its payload carries.

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError};
use andromeda_ngsi_ld_model::expanded::{
    entity_member::{ExpandedMemberError, extract_id, extract_types},
    node::expanded_nodes,
};
use serde_json::Value;

/// Refuse an expanded payload whose `@id` is not an identifier or one of whose
/// `@type` entries is not an entity type name.
///
/// A payload carrying no `@id` passes: an attribute fragment names its entity
/// in the request path, not in its body.
///
/// # Errors
///
/// Reports [`EntityError::Validation`] carrying
/// [`EntityValidationReason::ExpandedMember`] for the first node whose `@id`
/// or `@type` cannot be read.
pub(crate) fn validate_entity_identity(expanded: &Value) -> Result<(), EntityError> {
    expanded_nodes(expanded).iter().try_for_each(validate_node_identity)
}

/// Refuse one node whose `@id` or `@type` cannot be read.
fn validate_node_identity(node: &Value) -> Result<(), EntityError> {
    match extract_id(node) {
        Ok(_) | Err(ExpandedMemberError::MissingId) => {}
        Err(
            error @ (ExpandedMemberError::Id { .. }
            | ExpandedMemberError::TypeNotString
            | ExpandedMemberError::Type { .. }
            | ExpandedMemberError::AttributeName { .. }),
        ) => return Err(EntityError::Validation(EntityValidationReason::ExpandedMember(error))),
    }
    extract_types(node).map_err(|error| EntityError::Validation(EntityValidationReason::ExpandedMember(error)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{entity_identity_admission::validate_entity_identity, error::EntityError};
    use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
    use serde_json::json;

    #[test]
    fn a_fragment_without_an_identifier_passes() {
        validate_entity_identity(&json!([{"https://example.org/member": [{"@value": 1}]}])).expect("a fragment names no entity");
    }

    #[test]
    fn a_readable_identifier_and_types_pass() {
        validate_entity_identity(&json!([{"@id": "urn:ngsi-ld:Device:1", "@type": ["https://example.org/Device"]}])).expect("a well-formed entity");
    }

    #[test]
    fn every_node_of_the_payload_is_screened() {
        let long_id = format!("urn:ngsi-ld:Device:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let payload = json!([{"@id": "urn:ngsi-ld:Device:1"}, {"@id": long_id}]);
        let error = validate_entity_identity(&payload).expect_err("the second node's identifier is too long");
        assert!(matches!(error, EntityError::Validation(_)), "{error:?}");
    }
}
