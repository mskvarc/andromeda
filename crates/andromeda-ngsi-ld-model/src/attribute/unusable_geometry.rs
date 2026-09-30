//! The `GeoProperty` a decoded Entity carries only verbatim, because its
//! geometry is unusable.
//!
//! Entity decoding keeps an attribute it cannot read as
//! [`AttributeValue::Raw`] rather than failing the Entity. For a `GeoProperty`
//! that means one whose value is no geometry ETSI GS CIM 009 v1.9.1 clause 4.7
//! admits: a position off the WGS 84 axes of IETF RFC 7946 section 4, an
//! ordinate count section 3.1.1 gives no meaning, a `GeometryCollection`. A
//! write never stores one, because admission refuses it, but an Entity another
//! broker answers with arrives already decoded; a consumer that must not relay
//! such a geometry asks here which one it is.

use crate::{attribute::instance::AttributeValue, entity::Entity};
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry, geometry_type::GeometryType};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use serde_json::Value;
use std::{slice::from_ref, str::FromStr};
use thiserror::Error;

/// A `GeoProperty` whose geometry clause 4.7 gives the broker no reading of.
#[derive(Debug, Error)]
#[error("GeoProperty '{attribute}': {source}")]
pub struct UnusableGeometry {
    /// The attribute carrying the geometry.
    pub attribute: AttributeName,
    /// What makes the geometry unusable, down to the refused position.
    #[source]
    pub source: GeometryError,
}

/// The first `GeoProperty` `entity` holds only verbatim because its geometry
/// does not decode, whether written normalized (`"type": "GeoProperty"`) or
/// concise (the bare geometry of clause 4.7.3).
#[must_use]
pub fn first_unusable_geometry(entity: &Entity) -> Option<UnusableGeometry> {
    entity.attribute_members().find_map(|(attribute, value)| {
        let AttributeValue::Raw(raw) = value else {
            return None;
        };
        let instances = match raw {
            Value::Array(instances) => instances.as_slice(),
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => from_ref(other),
        };
        let source = instances
            .iter()
            .find_map(|instance| geometry_of(instance).and_then(|geometry| decode_geometry(geometry).err()))?;
        Some(UnusableGeometry {
            attribute: attribute.parse().ok()?,
            source,
        })
    })
}

/// The geometry one verbatim instance declares, if it declares one: the
/// `value` of a normalized `GeoProperty`, or the instance itself when it is a
/// concise one.
fn geometry_of(instance: &Value) -> Option<&Value> {
    let declared = instance.get("type").and_then(Value::as_str)?;
    if AttributeType::from_str(declared).is_ok_and(|family| family == AttributeType::GeoProperty) {
        return instance.get("value");
    }
    let concise = declared == "GeometryCollection" || GeometryType::from_str(declared).is_ok();
    (concise && (instance.get("coordinates").is_some() || instance.get("geometries").is_some())).then_some(instance)
}

/// Decode a geometry written as a `GeoJSON` object or, per clause 4.7.2, as a
/// string holding one.
fn decode_geometry(geometry: &Value) -> Result<Geometry, GeometryError> {
    match geometry {
        Value::String(document) => Geometry::from_geojson_value(&serde_json::from_str::<Value>(document)?),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => Geometry::from_geojson_value(geometry),
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute::unusable_geometry::first_unusable_geometry, entity::Entity};
    use andromeda_ngsi_ld_geometry::{geojson::GeometryError, position::PositionError};
    use serde_json::{Value, json};

    fn entity(attributes: &Value) -> Entity {
        let mut document = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"});
        if let (Some(target), Some(source)) = (document.as_object_mut(), attributes.as_object()) {
            target.extend(source.iter().map(|(key, value)| (key.clone(), value.clone()))); // clone: the fixture owns its members
        }
        serde_json::from_value(document).expect("an Entity decodes, unusable attributes kept verbatim")
    }

    #[test]
    fn a_normalized_geo_property_past_a_pole_is_found_with_its_position() {
        let found = first_unusable_geometry(&entity(&json!({
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.5, 146.05]}}
        })))
        .expect("latitude 146.05 is no position");
        pretty_assertions::assert_eq!(found.attribute.as_str(), "location");
        assert!(
            matches!(found.source, GeometryError::Position { source, .. } if source == PositionError::LatitudeOutOfRange { latitude: 146.05 }),
            "got {:?}",
            found.source
        );
        assert!(found.to_string().contains("latitude 146.05 is outside [-90, 90]"), "{found}");
    }

    #[test]
    fn a_concise_or_string_encoded_or_datasetid_geometry_past_the_axes_is_found() {
        for attributes in [
            json!({"location": {"type": "Point", "coordinates": [181, 0]}}),
            json!({"location": {"type": "GeoProperty", "value": "{\"type\":\"Point\",\"coordinates\":[181,0]}"}}),
            json!({"location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [181, 0]}, "datasetId": "urn:ngsi-ld:Dataset:a"}
            ]}),
        ] {
            assert!(first_unusable_geometry(&entity(&attributes)).is_some(), "{attributes}");
        }
    }

    #[test]
    fn usable_geometries_and_other_members_are_not_reported() {
        let found = first_unusable_geometry(&entity(&json!({
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [180, -90]}},
            "speed": {"type": "Property", "value": 10},
            "note": {"anything": "kept verbatim"}
        })));
        assert!(found.is_none(), "{found:?}");
    }
}
