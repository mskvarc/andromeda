//! Whether every `GeoProperty` an expanded write carries is a geometry the
//! broker can read back (ETSI GS CIM 009 v1.9.1 clause 5.5.4).
//!
//! Clause 4.7 makes a `GeoProperty` value a `GeoJSON` geometry per IETF RFC 7946.
//! Section 3.1.1 of that RFC gives a position exactly two ordinates (longitude,
//! latitude) or three (longitude, latitude, altitude), and section 4 places
//! them on the WGS 84 (CRS84) axes, longitude in [-180, 180] and latitude in
//! [-90, 90]. A position outside either is a payload the broker has no reading
//! of, and clause 5.5.4 makes it a `BadRequestData`, which is a thing to say at
//! the boundary rather than to discover in the store, or worse, to accept and
//! then read three different ways (planar, geography, in memory) afterwards.
//!
//! Every `GeoProperty` is checked, not only `location`, `observationSpace` and
//! `operationSpace`: clause 4.7.1 lets any attribute be a `GeoProperty`, and one
//! the broker cannot read is no more usable for being user-defined. Every
//! instance is checked too, since clause 4.5.5 lets an attribute carry several,
//! each keyed by `datasetId`, and each is read back as often as the default.
//!
//! A `GeometryCollection` is refused for a second, independent reason: clause
//! 4.7.1 allows a `GeoProperty` value every geometry RFC 7946 defines *except*
//! `GeometryCollection`, and clause 4.6.3 excludes it from reference geometries
//! too, so one is `BadRequestData` however well-formed it is.
//!
//! Mixed arity *within* one geometry is accepted and preserved verbatim. RFC
//! 7946 nowhere requires a uniform arity, and a `LineString` whose first position
//! is flat and whose second is elevated is a legal `GeoJSON` document.
//!
//! Every write path that stores a `GeoProperty` from expanded JSON-LD, the Core
//! Entity paths and the Temporal API create, append and instance-modification
//! paths alike, runs this one admission, so a geometry refused through one is
//! refused through all of them.

use crate::expanded::geo_node::validate_expanded_geo_node;
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use serde_json::Value;
use std::slice::from_ref;
use thiserror::Error;

/// Why an expanded write's `GeoProperty` cannot be stored.
#[derive(Debug, Error)]
pub enum GeoPropertyRefusal {
    /// A `GeoProperty` instance carried a geometry the broker has no reading of.
    #[error("GeoProperty '{attribute}': {source}")]
    Geometry {
        /// The `GeoProperty`, by its expanded IRI.
        attribute: AttributeName,
        /// What makes the geometry unusable, down to the refused position.
        #[source]
        source: GeometryError,
    },
    /// The member carrying the refused `GeoProperty` is not an attribute name.
    #[error("expanded attribute name is invalid: {0}")]
    AttributeName(#[source] NgsiLdReferenceError),
}

/// Reject an expanded write payload carrying a `GeoProperty` whose geometry has
/// an unusable position, or whose geometry is a `GeometryCollection`.
///
/// Accepts the shapes the write paths pass around: one expanded entity object,
/// or the array JSON-LD expansion produces. The payload may be a whole entity or
/// a patch fragment: a `GeoProperty` is validated wherever it appears, so create,
/// replace, update, partial update, merge, every batch form and every Temporal
/// API write are covered by the one reading.
///
/// # Errors
///
/// Returns the first [`GeoPropertyRefusal`], naming the `GeoProperty` and what
/// makes its geometry unusable.
pub fn validate_expanded_geo_properties(expanded: &Value) -> Result<(), GeoPropertyRefusal> {
    let entities = match expanded {
        Value::Array(entities) => entities.as_slice(),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => from_ref(other),
    };
    entities
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|entity| entity.iter())
        .filter(|(member, _)| !member.starts_with('@'))
        .filter_map(|(attribute_iri, value)| value.as_array().map(|instances| (attribute_iri, instances)))
        .try_for_each(|(attribute_iri, instances)| validate_instances(attribute_iri, instances))
}

/// Reject one expanded attribute instance, as a clause 5.6.14 instance
/// modification carries it, whose `GeoProperty` geometry is unusable.
///
/// # Errors
///
/// Returns the [`GeoPropertyRefusal`] naming `attribute_iri`.
pub fn validate_instance_geo_property(attribute_iri: &str, instance: &Value) -> Result<(), GeoPropertyRefusal> {
    validate_instances(attribute_iri, from_ref(instance))
}

/// Validate every `GeoProperty` instance of one attribute.
fn validate_instances(attribute_iri: &str, instances: &[Value]) -> Result<(), GeoPropertyRefusal> {
    instances
        .iter()
        .filter_map(Value::as_object)
        .filter(|instance| is_geo_property(instance.get("@type")))
        .flat_map(|instance| instance.get(properties::VALUE.as_str()).and_then(Value::as_array).into_iter().flatten())
        .try_for_each(|node| validate_geo_node(attribute_iri, node))
}

/// Whether an expanded attribute instance's `@type` names `GeoProperty`.
fn is_geo_property(types: Option<&Value>) -> bool {
    types
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(AttributeType::from_expanded_iri)
        .any(|attribute_type| attribute_type == AttributeType::GeoProperty)
}

/// Validate one `hasValue` node of a `GeoProperty`, in either of the two forms
/// clause 4.7 permits it to arrive in.
///
/// A literal `@value` is a whole `GeoJSON` document and is decoded outright, so
/// its geometry type and coordinates are checked against each other as well as
/// its positions. The fully expanded node form has no such decoder, and its
/// positions are checked in place by the same position rule.
fn validate_geo_node(attribute_iri: &str, node: &Value) -> Result<(), GeoPropertyRefusal> {
    match node.get("@value") {
        Some(Value::String(document)) => match serde_json::from_str::<Value>(document) {
            // A string that is not JSON at all is not this check's business:
            // it is a `hasValue` the attribute-envelope reading rejects.
            Err(_not_json) => Ok(()),
            Ok(parsed) => validate_geojson_document(attribute_iri, &parsed),
        },
        Some(document @ Value::Object(_)) => validate_geojson_document(attribute_iri, document),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_)) | None => {
            validate_expanded_geo_node(node).map_err(|source| refusal(attribute_iri, source))
        }
    }
}

/// Decode a literal `GeoJSON` document, reporting what makes it unusable.
///
/// Only documents shaped as a geometry object are decoded. Anything else is
/// left alone: refusing a `GeoProperty` for not being `GeoJSON` is a separate
/// reading of clause 5.5.4 from refusing it for its positions, and widening
/// this one to cover it would change what unrelated payloads are answered.
fn validate_geojson_document(attribute_iri: &str, document: &Value) -> Result<(), GeoPropertyRefusal> {
    let Some(object) = document.as_object() else {
        return Ok(());
    };
    if object.get("type").zip(object.get("coordinates")).is_none() && object.get("type").zip(object.get("geometries")).is_none() {
        return Ok(());
    }
    Geometry::from_geojson_value(document)
        .map(|_geometry| ())
        .map_err(|source| refusal(attribute_iri, source))
}

/// The refusal naming the `GeoProperty` whose geometry `source` refuses.
fn refusal(attribute_iri: &str, source: GeometryError) -> GeoPropertyRefusal {
    match attribute_iri.parse::<AttributeName>() {
        Ok(attribute) => GeoPropertyRefusal::Geometry { attribute, source },
        Err(name_error) => GeoPropertyRefusal::AttributeName(name_error),
    }
}

#[cfg(test)]
mod tests {
    use crate::expanded::geo_property_admission::{GeoPropertyRefusal, validate_expanded_geo_properties, validate_instance_geo_property};
    use andromeda_ngsi_ld_geometry::{coordinate_path::CoordinatePath, geojson::GeometryError, position::PositionError};
    use serde_json::{Value, json};

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const GEO_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/GeoProperty";
    const HAS_VALUE: &str = "https://uri.etsi.org/ngsi-ld/hasValue";

    /// One expanded `location` instance whose literal `@value` is `geojson`.
    fn location_instance(geojson: &Value) -> Value {
        json!({ "@type": [GEO_PROPERTY], HAS_VALUE: [{ "@value": geojson }] })
    }

    /// The refused position of a refusal, by where it sits and what is wrong.
    fn refused_position(refusal: &GeoPropertyRefusal) -> Option<(&str, &CoordinatePath, PositionError)> {
        match refusal {
            GeoPropertyRefusal::Geometry {
                attribute,
                source: GeometryError::Position { at, source },
            } => Some((attribute.as_str(), at, *source)),
            GeoPropertyRefusal::Geometry { .. } | GeoPropertyRefusal::AttributeName(_) => None,
        }
    }

    #[test]
    fn a_location_vertex_past_a_pole_is_refused_with_the_attribute_and_the_vertex() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [location_instance(&json!({
                "type": "Polygon",
                "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 95.0], [0.0, 0.0]]]
            }))]
        }]);
        let refusal = validate_expanded_geo_properties(&entity).expect_err("latitude 95 is no position");
        pretty_assertions::assert_eq!(
            refused_position(&refusal),
            Some((LOCATION, &CoordinatePath::new(vec![0, 2]), PositionError::LatitudeOutOfRange { latitude: 95.0 }))
        );
    }

    #[test]
    fn a_datasetid_instance_is_held_to_the_same_rule_as_the_default() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                location_instance(&json!({"type": "Point", "coordinates": [1.0, 2.0]})),
                {
                    "@type": [GEO_PROPERTY],
                    HAS_VALUE: [{ "@value": {"type": "Point", "coordinates": [181.0, 2.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
                }
            ]
        }]);
        let refusal = validate_expanded_geo_properties(&entity).expect_err("the tagged instance is off the axes");
        pretty_assertions::assert_eq!(
            refused_position(&refusal),
            Some((LOCATION, &CoordinatePath::default(), PositionError::LongitudeOutOfRange { longitude: 181.0 }))
        );
    }

    #[test]
    fn a_modified_instance_is_held_to_the_same_rule_as_a_whole_write() {
        let instance = location_instance(&json!({"type": "Point", "coordinates": [0.0, -90.5]}));
        let refusal = validate_instance_geo_property(LOCATION, &instance).expect_err("latitude -90.5 is no position");
        pretty_assertions::assert_eq!(
            refused_position(&refusal),
            Some((LOCATION, &CoordinatePath::default(), PositionError::LatitudeOutOfRange { latitude: -90.5 }))
        );
        assert!(validate_instance_geo_property(LOCATION, &location_instance(&json!({"type": "Point", "coordinates": [180.0, 90.0]}))).is_ok());
    }

    #[test]
    fn the_refusal_names_the_attribute_the_rule_and_the_position() {
        let entity = json!({ LOCATION: [location_instance(&json!({"type": "Point", "coordinates": [0.0, 91.0]}))] });
        let refusal = validate_expanded_geo_properties(&entity).expect_err("latitude 91 is no position");
        pretty_assertions::assert_eq!(
            refusal.to_string(),
            "GeoProperty 'https://uri.etsi.org/ngsi-ld/location': GeoJSON position latitude 91 is outside [-90, 90]; \
             RFC 7946 section 4 places positions in WGS 84 longitude and latitude, in decimal degrees (at coordinates)"
        );
    }
}
