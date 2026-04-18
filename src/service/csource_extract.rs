//! Service-side `Value` inspection for CSR creation and updates.
//!
//! The repository is forbidden from inspecting `Value`; the service layer
//! materialises GeoJSON strings from GeoProperty wrappers before any CSR
//! insert or update.

use serde_json::Value;

/// If `v` is a GeoProperty wrapper `{"type":"GeoProperty","value":{...}}`,
/// return the inner value's JSON string. Otherwise return the value as-is
/// (assumed to be raw GeoJSON).
pub fn unwrap_geo_property_to_string(v: &Value) -> String {
    if v.get("type").and_then(|t| t.as_str()) == Some("GeoProperty")
        && let Some(inner) = v.get("value")
    {
        return inner.to_string();
    }
    v.to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::unwrap_geo_property_to_string;
    use serde_json::json;

    #[test]
    fn geo_property_wrapper_returns_inner_value() {
        let input = json!({ "type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        let result = unwrap_geo_property_to_string(&input);
        let expected: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        pretty_assertions::assert_eq!(expected, json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn raw_geojson_returns_as_is() {
        let input = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        let result = unwrap_geo_property_to_string(&input);
        let expected: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        pretty_assertions::assert_eq!(expected, json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn non_geo_property_type_returns_full_value() {
        let input = json!({ "type": "Property", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        let result = unwrap_geo_property_to_string(&input);
        let expected: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        pretty_assertions::assert_eq!(expected, input);
    }
}
