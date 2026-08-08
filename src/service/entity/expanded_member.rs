//! Reading members out of an expanded JSON-LD entity, in the forms the write
//! path needs them.

use crate::error::AndromedaError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde_json::Value;

/// Every attribute name an expanded payload carries.
///
/// Keys that do not satisfy the NGSI-LD name grammar cannot name an attribute,
/// so they are skipped rather than reported as changed.
pub(crate) fn extract_attribute_keys(expanded: &Value) -> Vec<AttributeName> {
    let obj = match expanded {
        Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
        Value::Object(obj) => Some(obj),
        _other => None,
    };

    obj.map(|o| o.keys().filter(|k| !k.starts_with('@')).filter_map(|k| k.parse().ok()).collect())
        .unwrap_or_default()
}

/// The node object inside an expanded payload's array wrapper.
pub(crate) fn unwrap_expanded_ref(value: &Value) -> &Value {
    match value {
        Value::Array(arr) if !arr.is_empty() => &arr[0],
        other => other,
    }
}

/// Parse an expanded attribute IRI into the domain's attribute-name type.
///
/// The IRI comes straight out of JSON-LD term expansion, so a value that fails
/// the NGSI-LD name grammar means the caller asked for something that cannot
/// name an attribute at all.
pub(crate) fn attr_name_of(attr_iri: &str) -> Result<AttributeName, AndromedaError> {
    attr_iri
        .parse()
        .map_err(|err| AndromedaError::entity_validation(format!("Invalid attribute name '{attr_iri}': {err}")))
}
