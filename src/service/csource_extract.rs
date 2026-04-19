//! Service-side `Value` inspection for CSR creation and updates.
//!
//! The repository is forbidden from inspecting `Value`; the service layer
//! decodes inbound GeoProperty wrappers into canonical [`Wkb`] before any CSR
//! insert or update.

use crate::{core::error::AndromedaError, domain::geometry::Wkb};
use serde_json::Value;

/// If `v` is a GeoProperty wrapper `{"type":"GeoProperty","value":{...}}`,
/// decode the inner GeoJSON geometry. Otherwise treat `v` itself as a raw
/// GeoJSON geometry.
pub fn unwrap_geo_property_to_wkb(v: &Value) -> Result<Wkb, AndromedaError> {
    let geometry = if v.get("type").and_then(|t| t.as_str()) == Some("GeoProperty")
        && let Some(inner) = v.get("value")
    {
        inner
    } else {
        v
    };
    Wkb::from_geojson_value(geometry).map_err(|error| AndromedaError::csr_validation(format!("invalid GeoProperty geometry: {error}")))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::unwrap_geo_property_to_wkb;
    use crate::domain::geometry::Wkb;
    use serde_json::json;

    fn roundtrip(wkb: Wkb) -> serde_json::Value {
        serde_json::to_value(&wkb).expect("serialize")
    }

    #[test]
    fn geo_property_wrapper_returns_inner_value() {
        let input = json!({ "type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        let wkb = unwrap_geo_property_to_wkb(&input).expect("decode");
        pretty_assertions::assert_eq!(roundtrip(wkb), json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn raw_geojson_returns_as_is() {
        let input = json!({"type": "Point", "coordinates": [1.0, 2.0]});
        let wkb = unwrap_geo_property_to_wkb(&input).expect("decode");
        pretty_assertions::assert_eq!(roundtrip(wkb), json!({"type": "Point", "coordinates": [1.0, 2.0]}));
    }

    #[test]
    fn invalid_geometry_returns_error() {
        let input = json!({ "type": "Property", "value": {"type": "Point", "coordinates": [1.0, 2.0]} });
        assert!(unwrap_geo_property_to_wkb(&input).is_err());
    }
}
