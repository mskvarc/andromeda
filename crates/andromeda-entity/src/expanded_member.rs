//! Reading members out of an expanded JSON-LD entity, in the forms the write
//! path needs them.

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
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
