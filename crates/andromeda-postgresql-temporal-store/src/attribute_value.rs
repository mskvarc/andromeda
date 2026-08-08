//! The value carried by one expanded NGSI-LD attribute instance, and the
//! history columns it lands in.
//!
//! `attribute_history` stores a value in exactly one typed column so the
//! temporal query layer can filter and aggregate without parsing JSON. This
//! module owns the mapping from an expanded JSON-LD instance to that column.

use crate::{error::TemporalStoreError, value_type::ValueType};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::expanded::geo_property::expanded_geo_node_to_geometry;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The typed value columns of one `attribute_history` row.
///
/// Exactly one is populated; which one is decided by [`AttributeValue`].
pub struct HistoryValueColumns {
    /// Numeric value for number-typed properties.
    pub value_number: Option<f64>,
    /// String value for string-typed properties.
    pub value_string: Option<String>,
    /// Boolean value for boolean-typed properties.
    pub value_bool: Option<bool>,
    /// Geometry value for geo-typed properties.
    pub value_geo: Option<Geometry>,
    /// Arbitrary JSON value for object-typed or complex properties.
    pub value_object: Option<Value>,
    /// Datetime value for datetime-typed properties.
    pub value_datetime: Option<DateTime<Utc>>,
    /// URI string for relationship objects.
    pub value_uri: Option<String>,
}

/// The value of one attribute instance, resolved to the column it belongs in.
#[derive(Debug)]
pub enum AttributeValue {
    /// A JSON number.
    Number(f64),
    /// A JSON string that is not an `xsd:dateTime`.
    String(String),
    /// A JSON boolean.
    Boolean(bool),
    /// A GeoProperty geometry.
    Geometry(Geometry),
    /// A structured value: object, array, language map, JSON literal or list.
    Object(Value),
    /// An RFC 3339 timestamp, still in its lexical form.
    DateTime(String),
    /// A Relationship object IRI.
    Uri(String),
}

impl AttributeValue {
    /// The `ValueType` discriminant the attribute catalogue records for this value.
    pub const fn value_type(&self) -> ValueType {
        match self {
            AttributeValue::Number(_) => ValueType::Number,
            AttributeValue::String(_) => ValueType::String,
            AttributeValue::Boolean(_) => ValueType::Boolean,
            AttributeValue::Geometry(_) => ValueType::Geometry,
            AttributeValue::Object(_) => ValueType::Object,
            AttributeValue::DateTime(_) => ValueType::DateTime,
            AttributeValue::Uri(_) => ValueType::Uri,
        }
    }

    /// Spread this value across the history row's typed columns.
    pub fn into_columns(self) -> HistoryValueColumns {
        let mut columns = HistoryValueColumns {
            value_number: None,
            value_string: None,
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
        };
        match self {
            AttributeValue::Number(number) => columns.value_number = Some(number),
            AttributeValue::String(text) => columns.value_string = Some(text),
            AttributeValue::Boolean(flag) => columns.value_bool = Some(flag),
            AttributeValue::Geometry(geometry) => columns.value_geo = Some(geometry),
            AttributeValue::Object(object) => columns.value_object = Some(object),
            AttributeValue::DateTime(text) => {
                columns.value_datetime = DateTime::parse_from_rfc3339(&text).map(|dt| dt.with_timezone(&Utc)).ok();
            }
            AttributeValue::Uri(uri) => columns.value_uri = Some(uri),
        }
        columns
    }
}

/// Read the value of an expanded attribute instance, dispatching on its NGSI-LD type.
pub fn extract_attribute_value(instance: &Value, attribute_type: AttributeType) -> Result<AttributeValue, TemporalStoreError> {
    match attribute_type {
        AttributeType::Relationship => {
            let uri = instance
                .get(expanded_properties::OBJECT.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@id"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| TemporalStoreError::Validation("Relationship missing object".to_string()))?;
            Ok(AttributeValue::Uri(uri.to_string()))
        }
        AttributeType::GeoProperty => {
            let geo_node = instance
                .get(expanded_properties::VALUE.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| TemporalStoreError::Validation("GeoProperty missing value".to_string()))?;

            // Literal-`@value` form (string or already-compact object) keeps the
            // legacy decoding path. Expanded JSON-LD GeoJSON nodes go through the
            // direct walker, skipping the compact-string round-trip and
            // `extract_coordinates` clones that dominated the temporal hot path.
            let geometry = if let Some(val) = geo_node.get("@value") {
                let geo_value: Value = match val.as_str() {
                    Some(s) => {
                        serde_json::from_str(s).map_err(|error| TemporalStoreError::Validation(format!("Invalid GeoProperty GeoJSON string: {}", error)))?
                    }
                    None => val.clone(), // clone: need owned Value for from_geojson_value
                };
                Geometry::from_geojson_value(&geo_value).map_err(|error| TemporalStoreError::Validation(format!("Invalid GeoProperty geometry: {}", error)))?
            } else {
                expanded_geo_node_to_geometry(geo_node).ok_or_else(|| TemporalStoreError::Validation("Invalid expanded GeoProperty geometry".to_string()))?
            };
            Ok(AttributeValue::Geometry(geometry))
        }
        AttributeType::VocabProperty => {
            let arr = instance
                .get(expanded_properties::VOCAB.as_str())
                .and_then(|v| v.as_array())
                .ok_or_else(|| TemporalStoreError::Validation("VocabProperty missing vocab value".to_string()))?;
            if arr.len() > 1 {
                let values: Vec<Value> = arr.iter().filter_map(|obj| obj.get("@id").or(obj.get("@value")).cloned()).collect(); // clone: need owned Values for collected Vec
                Ok(AttributeValue::Object(Value::Array(values)))
            } else {
                let vocab_val = arr
                    .first()
                    .and_then(|obj| obj.get("@id").or(obj.get("@value")))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| TemporalStoreError::Validation("VocabProperty missing vocab value".to_string()))?;
                Ok(AttributeValue::String(vocab_val.to_string()))
            }
        }
        AttributeType::LanguageProperty => {
            let lang_map = instance
                .get(expanded_properties::LANGUAGE_MAP.as_str())
                .ok_or_else(|| TemporalStoreError::Validation("LanguageProperty missing languageMap".to_string()))?;
            Ok(AttributeValue::Object(lang_map.clone())) // clone: need owned Value for the enum payload; lang_map is borrowed from instance
        }
        AttributeType::JsonProperty => {
            let json_val = instance
                .get(expanded_properties::JSON.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@value"))
                .ok_or_else(|| TemporalStoreError::Validation("JsonProperty missing json value".to_string()))?;
            Ok(AttributeValue::Object(json_val.clone())) // clone: need owned Value for the enum payload; json_val is borrowed from instance
        }
        AttributeType::ListProperty => {
            let list = instance
                .get(expanded_properties::VALUE_LIST.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@list"))
                .ok_or_else(|| TemporalStoreError::Validation("ListProperty missing valueList".to_string()))?;
            Ok(AttributeValue::Object(list.clone())) // clone: need owned Value for the enum payload; list is borrowed from instance
        }
        AttributeType::ListRelationship => {
            let list = instance
                .get(expanded_properties::OBJECT_LIST.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@list"))
                .ok_or_else(|| TemporalStoreError::Validation("ListRelationship missing objectList".to_string()))?;
            let uris: Vec<Value> = list
                .as_array()
                .map(|arr| arr.iter().filter_map(|obj| obj.get("@id").cloned()).collect()) // clone: need owned Values for collected Vec
                .unwrap_or_default();
            Ok(AttributeValue::Object(Value::Array(uris)))
        }
        AttributeType::Property => {
            let has_value = instance
                .get(expanded_properties::VALUE.as_str())
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| TemporalStoreError::Validation("Property missing value".to_string()))?;

            if let Some(value) = has_value.get("@value") {
                let type_hint = has_value.get("@type").and_then(|t| t.as_str());

                if (type_hint == Some(expanded_classes::DATE_TIME.as_str()) || type_hint == Some(expanded_properties::XSD_DATETIME.as_str()))
                    && let Some(s) = value.as_str()
                {
                    return Ok(AttributeValue::DateTime(s.to_string()));
                }

                match value {
                    Value::Number(n) => Ok(AttributeValue::Number(n.as_f64().unwrap_or(0.0))),
                    Value::Bool(b) => Ok(AttributeValue::Boolean(*b)),
                    Value::String(s) => Ok(AttributeValue::String(s.clone())), // clone: need owned String for the enum payload; s is borrowed from JSON
                    Value::Object(_) | Value::Array(_) => Ok(AttributeValue::Object(value.clone())), // clone: need owned Value for the enum payload; value is borrowed from JSON
                    Value::Null => Ok(AttributeValue::String("null".to_string())),
                }
            } else {
                // Expanded JSON-LD node without @value -- store as Object
                Ok(AttributeValue::Object(has_value.clone())) // clone: need owned Value for the enum payload; has_value is borrowed from instance
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_datetime_typed_property_lands_in_the_datetime_column() {
        let instance = serde_json::json!({
            expanded_properties::VALUE.as_str(): [{
                "@value": "2026-07-25T10:00:00Z",
                "@type": expanded_classes::DATE_TIME.as_str(),
            }]
        });
        let value = extract_attribute_value(&instance, AttributeType::Property).expect("datetime property");
        pretty_assertions::assert_eq!(value.value_type(), ValueType::DateTime);
        let columns = value.into_columns();
        assert!(columns.value_datetime.is_some());
        assert!(columns.value_string.is_none());
    }

    #[test]
    fn a_numeric_property_lands_in_the_number_column() {
        let instance = serde_json::json!({ expanded_properties::VALUE.as_str(): [{"@value": 12.5}] });
        let columns = extract_attribute_value(&instance, AttributeType::Property)
            .expect("numeric property")
            .into_columns();
        pretty_assertions::assert_eq!(columns.value_number, Some(12.5));
    }

    #[test]
    fn a_relationship_lands_in_the_uri_column() {
        let instance = serde_json::json!({
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Entity:target"}]
        });
        let columns = extract_attribute_value(&instance, AttributeType::Relationship)
            .expect("relationship")
            .into_columns();
        pretty_assertions::assert_eq!(columns.value_uri.as_deref(), Some("urn:ngsi-ld:Entity:target"));
    }

    #[test]
    fn a_property_with_no_value_is_rejected() {
        assert!(extract_attribute_value(&serde_json::json!({}), AttributeType::Property).is_err());
    }
}
