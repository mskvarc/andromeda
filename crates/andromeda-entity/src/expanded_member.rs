//! Reading members out of an expanded JSON-LD entity, in the forms the write
//! path needs them.

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::expanded::entity_member::{ExpandedMemberError, extract_attribute_names, extract_types};
use serde_json::Value;

/// The node object inside an expanded payload's array wrapper.
pub(crate) fn unwrap_expanded_ref(value: &Value) -> &Value {
    match value {
        Value::Array(arr) if !arr.is_empty() => &arr[0],
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) | Value::Object(_)) => other,
    }
}

/// Parse an expanded attribute IRI into the domain's attribute-name type.
///
/// The IRI comes straight out of JSON-LD term expansion, so a value that fails
/// the NGSI-LD name grammar means the caller asked for something that cannot
/// name an attribute at all.
pub(crate) fn attr_name_of(attr_iri: &str) -> Result<AttributeName, EntityError> {
    attr_iri.parse().map_err(|err| {
        EntityError::Validation(EntityValidationReason::AttributeName {
            iri: attr_iri.to_string(),
            source: err,
        })
    })
}

/// A member of a payload the write path built from an admitted request that
/// cannot be read: clause 5.5.4 makes it `BadRequestData`, whichever step of
/// the write finds it.
const fn written_member_fault(error: ExpandedMemberError) -> EntityError {
    EntityError::Validation(EntityValidationReason::ExpandedMember(error))
}

/// The entity types of a payload the write path built from an admitted
/// request.
///
/// # Errors
///
/// Reports [`EntityError::Validation`] for a type that cannot be read.
pub(crate) fn written_entity_types(payload: &Value) -> Result<Vec<EntityTypeName>, EntityError> {
    extract_types(payload).map_err(written_member_fault)
}

/// The attribute names of a payload the write path built from an admitted
/// request.
///
/// # Errors
///
/// Reports [`EntityError::Validation`] for a member key that cannot be read.
pub(crate) fn written_attribute_names(payload: &Value) -> Result<Vec<AttributeName>, EntityError> {
    extract_attribute_names(payload).map_err(written_member_fault)
}

/// The entity types of a payload read back from the store.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] for a type that cannot be read: every
/// stored payload was admitted when it was written.
pub(crate) fn stored_entity_types(payload: &Value) -> Result<Vec<EntityTypeName>, EntityError> {
    extract_types(payload).map_err(EntityError::internal)
}

/// The attribute names of a payload read back from the store.
///
/// # Errors
///
/// Reports [`EntityError::Internal`] for a member key that cannot be read.
pub(crate) fn stored_attribute_names(payload: &Value) -> Result<Vec<AttributeName>, EntityError> {
    extract_attribute_names(payload).map_err(EntityError::internal)
}
