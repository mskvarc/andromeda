//! Readers for the entity-level members of an expanded payload: `@id`,
//! `@type`, `scope` and `expiresAt`. A named GeoProperty's geometry is read by
//! [`crate::expanded::geo_property::extract_geo_property_geometry`], the single
//! reader shared by the write/index and read/match paths.

use crate::expanded::node::expanded_object_ref;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, scope::grammar::ScopeBuf};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Extract the `@id` of an expanded entity payload.
pub fn extract_id(expanded: &Value) -> Option<&str> {
    expanded_object_ref(expanded).and_then(|obj| obj.get("@id")).and_then(Value::as_str)
}

/// Extract `@type` IRIs from an expanded entity payload.
///
/// Entries that are not strings, or that do not satisfy the NGSI-LD name
/// grammar, are skipped: a payload member that cannot name a type is not one.
pub fn extract_types(expanded: &Value) -> Vec<EntityTypeName> {
    expanded_object_ref(expanded)
        .and_then(|obj| obj.get("@type"))
        .and_then(|t| t.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).filter_map(|raw| raw.parse().ok()).collect())
        .unwrap_or_default()
}

/// Extract scope strings from an expanded entity payload.
pub fn extract_scopes(expanded: &Value) -> Vec<ScopeBuf> {
    let Some(obj) = expanded_object_ref(expanded) else {
        return Vec::new();
    };
    let Some(scope_value) = obj.get(properties::SCOPE.as_str()) else {
        return Vec::new();
    };
    match scope_value {
        Value::Array(arr) => arr
            .iter()
            // A value that does not satisfy the clause 4.18 scope grammar
            // cannot name a scope, so it is skipped rather than stored as one.
            .filter_map(|v| v.as_object().and_then(|obj| obj.get("@value")).and_then(|v| v.as_str()))
            .filter_map(|raw| ScopeBuf::new(raw.to_string()).ok())
            .collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => Vec::new(),
    }
}

/// Extract the `expiresAt` value from an expanded JSON-LD entity payload
/// (ETSI GS CIM 009 clause 4.22).
pub fn extract_expires_at(expanded: &Value) -> Option<DateTime<Utc>> {
    let obj = expanded_object_ref(expanded)?;
    let dt_str = obj
        .get(properties::EXPIRES_AT.as_str())?
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@value"))
        .and_then(|v| v.as_str())?;
    DateTime::parse_from_rfc3339(dt_str).ok().map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use crate::expanded::entity_member::{extract_expires_at, extract_id, extract_scopes};
    use chrono::Datelike;
    use serde_json::json;

    #[test]
    fn the_identifier_is_read_off_the_expanded_node() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        pretty_assertions::assert_eq!(extract_id(&entity), Some("urn:ngsi-ld:Building:001"));
    }

    #[test]
    fn an_expanded_payload_without_an_identifier_yields_none() {
        assert!(extract_id(&json!([{"@type": ["https://example.com/Building"]}])).is_none());
    }

    #[test]
    fn extract_scopes_from_entity() {
        let entity = json!([{
            "@type": ["https://example.com/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [
                {"@value": "/Madrid"},
                {"@value": "/Traffic"}
            ]
        }]);
        let scopes = extract_scopes(&entity);
        pretty_assertions::assert_eq!(scopes.iter().map(|scope| scope.as_str()).collect::<Vec<&str>>(), vec!["/Madrid", "/Traffic"]);
    }

    #[test]
    fn extract_expires_at_from_expanded_entity() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "2030-12-31T23:59:59.000Z"
            }]
        }]);
        let result = extract_expires_at(&entity);
        assert!(result.is_some());
        pretty_assertions::assert_eq!(result.unwrap().year(), 2030);
    }

    #[test]
    fn extract_expires_at_missing() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "@type": ["https://example.com/Building"]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }

    #[test]
    fn extract_expires_at_invalid_date() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{
                "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
                "@value": "not-a-date"
            }]
        }]);
        assert!(extract_expires_at(&entity).is_none());
    }
}
