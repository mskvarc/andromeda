//! Service-side `Value` inspection for CSR creation and updates.
//!
//! The repository is forbidden from inspecting `Value`; the service layer
//! decodes inbound GeoProperty wrappers into a [`Geometry`] before any CSR
//! insert or update.

use crate::core::error::AndromedaError;
use andromeda_domain::geometry::Geometry;
use serde_json::Value;

/// If `v` is a GeoProperty wrapper `{"type":"GeoProperty","value":{...}}`,
/// decode the inner GeoJSON geometry. Otherwise treat `v` itself as a raw
/// GeoJSON geometry.
pub fn unwrap_geo_property_to_geometry(v: &Value) -> Result<Geometry, AndromedaError> {
    let geometry = if v.get("type").and_then(|t| t.as_str()) == Some("GeoProperty")
        && let Some(inner) = v.get("value")
    {
        inner
    } else {
        v
    };
    Geometry::from_geojson_value(geometry).map_err(|error| AndromedaError::csr_validation(format!("invalid GeoProperty geometry: {error}")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::unwrap_geo_property_to_geometry;
    use andromeda_domain::geometry::Geometry;
    use serde_json::json;

    fn roundtrip(geometry: Geometry) -> serde_json::Value {
        serde_json::to_value(&geometry).expect("serialize")
    }

    #[test]
    fn geo_property_wrapper_returns_inner_value() {
        let input = json!({ "type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        let geometry = unwrap_geo_property_to_geometry(&input).expect("decode");
        pretty_assertions::assert_eq!(roundtrip(geometry), json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn raw_geojson_returns_as_is() {
        let input = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        let geometry = unwrap_geo_property_to_geometry(&input).expect("decode");
        pretty_assertions::assert_eq!(roundtrip(geometry), json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn invalid_geometry_returns_error() {
        let input = json!({ "type": "Property", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        assert!(unwrap_geo_property_to_geometry(&input).is_err());
    }
}
