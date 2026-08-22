//! Reading back the names the broker itself stored.
//!
//! A stored type name, attribute name or discovery IRI came out of the broker's
//! own store or its own compactor, so one that does not parse is an internal
//! fault rather than a client error.

use crate::error::EntityError;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use iri_rs::IriBuf;
use serde_json::Value;
use std::str::FromStr;

/// Parse a stored entity-type name.
pub(super) fn parse_type_name(value: &str) -> Result<EntityTypeName, EntityError> {
    EntityTypeName::from_str(value).map_err(|error| EntityError::internal(format!("stored entity type name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored attribute name.
pub(super) fn parse_attribute_name(value: &str) -> Result<AttributeName, EntityError> {
    AttributeName::from_str(value).map_err(|error| EntityError::internal(format!("stored attribute name is not a valid NGSI-LD name: {error}")))
}

/// Parse a stored expanded IRI.
pub(super) fn parse_discovery_iri(value: String) -> Result<IriBuf, EntityError> {
    IriBuf::new(value).map_err(|error| EntityError::internal(format!("stored discovery identifier is not a valid IRI: {error}")))
}

/// The clause 4.5 attribute families an expanded attribute node declares in
/// its `@type`, or `None` when it declares none the broker knows.
pub(super) fn extract_attribute_types(value: &Value) -> Option<Vec<AttributeType>> {
    let obj = match value {
        Value::Array(arr) => arr.first()?.as_object()?,
        Value::Object(obj) => obj,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => return None,
    };

    obj.get("@type").and_then(|t| match t {
        Value::Array(arr) => {
            let types: Vec<AttributeType> = arr.iter().filter_map(|v| v.as_str()).filter_map(AttributeType::from_iri_suffix).collect();
            if types.is_empty() { None } else { Some(types) }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_expanded_attribute_node_yields_its_families() {
        let node = json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]);
        pretty_assertions::assert_eq!(extract_attribute_types(&node), Some(vec![AttributeType::Property]));
        assert!(extract_attribute_types(&json!([{"@type": ["https://example.com/Unknown"]}])).is_none());
        assert!(extract_attribute_types(&json!("not a node")).is_none());
    }

    #[test]
    fn a_stored_name_that_does_not_parse_is_an_internal_fault() {
        assert!(matches!(parse_type_name("two words"), Err(EntityError::Internal { .. })));
        assert!(matches!(parse_attribute_name(""), Err(EntityError::Internal { .. })));
        assert!(matches!(parse_discovery_iri("not an iri".to_owned()), Err(EntityError::Internal { .. })));
    }
}
