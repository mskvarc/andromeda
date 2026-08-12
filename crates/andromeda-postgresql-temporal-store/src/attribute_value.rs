//! The value carried by one expanded NGSI-LD attribute instance, and the
//! history columns it lands in.
//!
//! `attribute_history` stores a value in exactly one typed column so the
//! temporal query layer can filter and aggregate without parsing JSON. This
//! module owns the mapping from an expanded JSON-LD instance to that column.

use crate::error::TemporalStoreError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_type::AttributeType,
    ngsi_ld_date::NgsiLdDate,
    ngsi_ld_date_time::NgsiLdDateTime,
    ngsi_ld_time::NgsiLdTime,
    uri::Uri,
};
use andromeda_ngsi_ld_model::{attribute::json_number::JsonNumber, expanded::geo_property::expanded_geo_node_to_geometry};
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// The typed value columns of one `attribute_history` row.
///
/// Exactly one is populated; which one is decided by [`AttributeValue`].
pub struct HistoryValueColumns {
    /// Decimal literal for the arbitrary-precision numeric column.
    ///
    /// The column is `NUMERIC`, so the value is handed over as its decimal text:
    /// an integer beyond the range the double format names individually reaches
    /// storage unchanged rather than being refused or rounded at the boundary.
    pub value_number: Option<String>,
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
    pub value_uri: Option<Uri>,
}

/// The value of one attribute instance, resolved to the column it belongs in.
#[derive(Debug)]
pub enum AttributeValue {
    /// A JSON number, in the shape it was parsed in.
    Number(JsonNumber),
    /// A JSON string that is not an `xsd:dateTime`.
    String(String),
    /// A JSON boolean.
    Boolean(bool),
    /// A GeoProperty geometry.
    Geometry(Geometry),
    /// A structured value: object, array, language map, JSON literal or list.
    Object(Value),
    /// An RFC 3339 timestamp, already normalised to the broker's microsecond precision.
    DateTime(NgsiLdDateTime),
    /// A Relationship object IRI.
    Uri(Uri),
}

impl AttributeValue {
    /// Spread this value across the history row's typed columns.
    ///
    /// Infallible by construction: every value that can fail to reach a column
    /// has already been rejected by [`extract_attribute_value`], so no branch
    /// here can silently leave a column empty.
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
            AttributeValue::Number(number) => columns.value_number = Some(number.to_string()),
            AttributeValue::String(text) => columns.value_string = Some(text),
            AttributeValue::Boolean(flag) => columns.value_bool = Some(flag),
            AttributeValue::Geometry(geometry) => columns.value_geo = Some(geometry),
            AttributeValue::Object(object) => columns.value_object = Some(object),
            AttributeValue::DateTime(instant) => columns.value_datetime = Some(instant.get()),
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
            let uri = uri
                .parse::<Uri>()
                .map_err(|error| TemporalStoreError::Validation(format!("Relationship object is not a valid URI: {error}")))?;
            Ok(AttributeValue::Uri(uri))
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

                if let Some(text) = value.as_str()
                    && let Some(typed) = temporal_attribute_value(type_hint, text)
                {
                    return typed;
                }

                match value {
                    // The history column is `NUMERIC`, so the number keeps the
                    // shape it was parsed in and reaches storage as its decimal
                    // literal: a clause 5.6.14 modification stores exactly the
                    // values a creation does.
                    Value::Number(number) => Ok(AttributeValue::Number(JsonNumber::try_from(number)?)),
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

/// Decode a Property value the client typed as one of the three clause 4.6.3
/// temporal forms, or `None` when the declared type is not one of them.
///
/// A `DateTime` reaches its own column. A `Time` and a `Date` reach the string
/// column, and are stored in the canonical rendering their type defines rather
/// than verbatim: clause 4.6.3 admits many spellings of one `Time`, clause 4.9
/// orders `Time` and `Date` values chronologically, and byte-wise order over the
/// string column is chronological order only for a fixed-width canonical form.
fn temporal_attribute_value(type_hint: Option<&str>, raw: &str) -> Option<Result<AttributeValue, TemporalStoreError>> {
    let hint = type_hint?;
    if hint == expanded_classes::DATE_TIME.as_str() || hint == expanded_properties::XSD_DATETIME.as_str() {
        return Some(raw.parse::<NgsiLdDateTime>().map(AttributeValue::DateTime).map_err(TemporalStoreError::from));
    }
    if hint == expanded_classes::TIME.as_str() {
        return Some(
            raw.parse::<NgsiLdTime>()
                .map(|time| AttributeValue::String(time.to_string()))
                .map_err(|error| TemporalStoreError::Validation(format!("invalid Time property '{raw}': {error}"))),
        );
    }
    if hint == expanded_classes::DATE.as_str() {
        return Some(
            raw.parse::<NgsiLdDate>()
                .map(|date| AttributeValue::String(date.to_string()))
                .map_err(|error| TemporalStoreError::Validation(format!("invalid Date property '{raw}': {error}"))),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One expanded Property instance carrying a typed literal value.
    fn typed_property(type_iri: &str, value: &str) -> Value {
        serde_json::json!({
            expanded_properties::VALUE.as_str(): [{"@value": value, "@type": type_iri}]
        })
    }

    #[test]
    fn a_time_typed_property_is_canonicalised_before_it_reaches_the_string_column() {
        // Clause 4.6.3 admits any offset; the string column orders byte-wise, so
        // only one fixed-width UTC rendering may be stored.
        let columns = extract_attribute_value(&typed_property(expanded_classes::TIME.as_str(), "13:00:00+01:00"), AttributeType::Property)
            .expect("a Time property")
            .into_columns();
        pretty_assertions::assert_eq!(columns.value_string.as_deref(), Some("12:00:00.000000Z"));
    }

    #[test]
    fn a_time_typed_property_that_is_not_a_full_time_is_refused() {
        let error = extract_attribute_value(&typed_property(expanded_classes::TIME.as_str(), "12:00:00"), AttributeType::Property)
            .expect_err("RFC 3339 full-time requires an offset");
        assert!(matches!(error, TemporalStoreError::Validation(_)), "got {error:?}");
    }

    #[test]
    fn a_date_typed_property_is_validated_before_it_reaches_the_string_column() {
        let columns = extract_attribute_value(&typed_property(expanded_classes::DATE.as_str(), "2026-04-12"), AttributeType::Property)
            .expect("a Date property")
            .into_columns();
        pretty_assertions::assert_eq!(columns.value_string.as_deref(), Some("2026-04-12"));
        let error = extract_attribute_value(&typed_property(expanded_classes::DATE.as_str(), "2026-4-2"), AttributeType::Property)
            .expect_err("an unpadded spelling would not sort chronologically");
        assert!(matches!(error, TemporalStoreError::Validation(_)), "got {error:?}");
    }

    #[test]
    fn a_datetime_typed_property_lands_in_the_datetime_column() {
        let instance = serde_json::json!({
            expanded_properties::VALUE.as_str(): [{
                "@value": "2026-07-25T10:00:00Z",
                "@type": expanded_classes::DATE_TIME.as_str(),
            }]
        });
        let value = extract_attribute_value(&instance, AttributeType::Property).expect("datetime property");
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
        pretty_assertions::assert_eq!(columns.value_number.as_deref(), Some("12.5"));
    }

    #[test]
    fn a_relationship_lands_in_the_uri_column() {
        let instance = serde_json::json!({
            expanded_properties::OBJECT.as_str(): [{"@id": "urn:ngsi-ld:Entity:target"}]
        });
        let columns = extract_attribute_value(&instance, AttributeType::Relationship)
            .expect("relationship")
            .into_columns();
        pretty_assertions::assert_eq!(columns.value_uri.as_ref().map(Uri::as_str), Some("urn:ngsi-ld:Entity:target"));
    }

    #[test]
    fn a_property_with_no_value_is_rejected() {
        assert!(extract_attribute_value(&serde_json::json!({}), AttributeType::Property).is_err());
    }

    /// One above 2^53, which the double format cannot name. Clause 5.6.14
    /// modifies an instance in place, and the value it can store must be the
    /// same set a creation can store: the `NUMERIC` column takes both.
    #[test]
    fn an_integer_with_no_exact_double_reaches_the_numeric_column_unchanged() {
        let instance: Value = serde_json::from_str(&format!(r#"{{"{}": [{{"@value": 9007199254740993}}]}}"#, expanded_properties::VALUE.as_str()))
            .expect("a well-formed expanded property");
        let columns = extract_attribute_value(&instance, AttributeType::Property)
            .expect("an arbitrary-precision integer")
            .into_columns();
        pretty_assertions::assert_eq!(columns.value_number.as_deref(), Some("9007199254740993"));
    }

    #[test]
    fn a_large_integer_that_the_double_format_names_exactly_is_still_accepted() {
        let instance: Value = serde_json::from_str(&format!(r#"{{"{}": [{{"@value": 9007199254740992}}]}}"#, expanded_properties::VALUE.as_str()))
            .expect("a well-formed expanded property");
        let columns = extract_attribute_value(&instance, AttributeType::Property)
            .expect("2^53 has an exact double")
            .into_columns();
        pretty_assertions::assert_eq!(columns.value_number.as_deref(), Some("9007199254740992"));
    }

    #[test]
    fn a_sub_microsecond_datetime_is_truncated_before_it_reaches_the_column() {
        let instance = serde_json::json!({
            expanded_properties::VALUE.as_str(): [{
                "@value": "2026-07-25T10:00:00.1234569Z",
                "@type": expanded_classes::DATE_TIME.as_str(),
            }]
        });
        let columns = extract_attribute_value(&instance, AttributeType::Property)
            .expect("a sub-microsecond datetime property")
            .into_columns();
        let stored = columns.value_datetime.expect("the datetime column is populated");
        pretty_assertions::assert_eq!(stored.timestamp_subsec_nanos(), 123_456_000);
    }

    #[test]
    fn an_unparsable_datetime_is_rejected_rather_than_stored_as_null() {
        let instance = serde_json::json!({
            expanded_properties::VALUE.as_str(): [{
                "@value": "yesterday",
                "@type": expanded_classes::DATE_TIME.as_str(),
            }]
        });
        let error = extract_attribute_value(&instance, AttributeType::Property).expect_err("the value is not a timestamp");
        assert!(matches!(error, TemporalStoreError::TimestampPrecision(_)), "got {error:?}");
    }
}
