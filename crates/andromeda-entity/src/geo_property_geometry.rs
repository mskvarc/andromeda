//! Checking that every GeoProperty a write carries is a geometry the broker can
//! read back (ETSI GS CIM 009 v1.9.1 clause 5.5.4).
//!
//! Clause 4.7 makes a GeoProperty value a GeoJSON geometry per IETF RFC 7946,
//! and section 3.1.1 of that RFC gives a position exactly two ordinates
//! (longitude, latitude) or three (longitude, latitude, altitude). It permits
//! further elements but assigns them no meaning, and NGSI-LD assigns them none
//! either, so a four-ordinate position is a payload the broker has no reading
//! of. Clause 5.5.4 makes that a `BadRequestData`, which is a thing to say at
//! the boundary rather than to discover in the store — or, worse, to accept and
//! then silently skip on every subsequent read.
//!
//! Every GeoProperty is checked, not only `location`, `observationSpace` and
//! `operationSpace`: clause 4.7.1 lets any attribute be a GeoProperty, and one
//! the broker cannot read is no more usable for being user-defined.
//!
//! A GeometryCollection is refused for a second, independent reason: clause
//! 4.7.1 allows a GeoProperty value every geometry RFC 7946 defines *except*
//! GeometryCollection, and clause 4.6.3 excludes it from reference geometries
//! too, so one is `BadRequestData` however well-formed it is.
//!
//! Mixed arity *within* one geometry is accepted and preserved verbatim. RFC
//! 7946 nowhere requires a uniform arity, and a LineString whose first position
//! is flat and whose second is elevated is a legal GeoJSON document.

use crate::error::EntityError;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::expanded::geo_property::validate_expanded_geo_node;
use andromeda_ngsi_ld_vocabulary::{expanded::properties as expanded_properties, expanded_to_compact};
use serde_json::Value;
use std::{fmt::Display, slice::from_ref};

/// Reject an expanded write payload carrying a GeoProperty whose geometry has an
/// unusable position, or whose geometry is a GeometryCollection.
///
/// Accepts the shapes the rest of the write path passes around: one expanded
/// entity object, or the array JSON-LD expansion produces. The payload may be a
/// whole entity or a patch fragment — a GeoProperty is validated wherever it
/// appears, so create, replace, update, partial-update, merge and every batch
/// form are covered by the one reading.
///
/// # Errors
///
/// Returns [`EntityError::Validation`], which the HTTP layer renders as a 400
/// `BadRequestData`, naming the GeoProperty and what makes its geometry
/// unusable.
pub(crate) fn validate_geo_property_geometries(expanded: &Value) -> Result<(), EntityError> {
    let entities = match expanded {
        Value::Array(entities) => entities.as_slice(),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => from_ref(other),
    };

    for entity in entities {
        let Some(object) = entity.as_object() else {
            continue;
        };
        for (attribute_iri, value) in object {
            if attribute_iri.starts_with('@') {
                continue;
            }
            validate_attribute(attribute_iri, value)?;
        }
    }
    Ok(())
}

/// Validate every GeoProperty instance of one attribute.
///
/// Clause 4.5.5 lets an attribute carry several instances, each keyed by
/// `datasetId`; every one of them is a geometry a later read has to cope with,
/// so every one of them is checked rather than only the default.
fn validate_attribute(attribute_iri: &str, value: &Value) -> Result<(), EntityError> {
    let Value::Array(instances) = value else {
        return Ok(());
    };

    for instance in instances {
        let Some(object) = instance.as_object() else {
            continue;
        };
        if !is_geo_property(object.get("@type")) {
            continue;
        }
        let values = object.get(expanded_properties::VALUE.as_str()).and_then(Value::as_array);
        for node in values.into_iter().flatten() {
            validate_geo_node(attribute_iri, node)?;
        }
    }
    Ok(())
}

/// Whether an expanded attribute instance's `@type` names GeoProperty.
fn is_geo_property(types: Option<&Value>) -> bool {
    types
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(AttributeType::from_expanded_iri)
        .any(|attribute_type| attribute_type == AttributeType::GeoProperty)
}

/// Validate one `hasValue` node of a GeoProperty, in either of the two forms
/// clause 4.7 permits it to arrive in.
///
/// A literal `@value` is a whole GeoJSON document and is decoded outright, so
/// its geometry type and coordinates are checked against each other as well as
/// its arities. The fully expanded node form has no such decoder, and its
/// positions are checked in place.
fn validate_geo_node(attribute_iri: &str, node: &Value) -> Result<(), EntityError> {
    match node.get("@value") {
        Some(Value::String(document)) => match serde_json::from_str::<Value>(document) {
            // A string that is not JSON at all is not this check's business:
            // it is a `hasValue` the attribute-envelope reading rejects.
            Err(_not_json) => Ok(()),
            Ok(parsed) => validate_geojson_document(attribute_iri, &parsed),
        },
        Some(document @ Value::Object(_)) => validate_geojson_document(attribute_iri, document),
        Some(Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_)) | None => {
            validate_expanded_geo_node(node).map_err(|error| geo_property_error(attribute_iri, &error))
        }
    }
}

/// Decode a literal GeoJSON document, reporting what makes it unusable.
///
/// Only documents shaped as a geometry object are decoded. Anything else is
/// left alone: refusing a GeoProperty for not being GeoJSON is a separate
/// reading of clause 5.5.4 from refusing it for an ordinate count, and widening
/// this one to cover it would change what unrelated payloads are answered.
fn validate_geojson_document(attribute_iri: &str, document: &Value) -> Result<(), EntityError> {
    let Some(object) = document.as_object() else {
        return Ok(());
    };
    if object.get("type").zip(object.get("coordinates")).is_none() && object.get("type").zip(object.get("geometries")).is_none() {
        return Ok(());
    }
    Geometry::from_geojson_value(document)
        .map(|_geometry| ())
        .map_err(|error| geo_property_error(attribute_iri, &error))
}

/// The clause 5.5.4 `BadRequestData` detail for one unusable GeoProperty.
///
/// The GeoProperty is named by its compact term where the core context defines
/// one, and by its expanded IRI otherwise: a user-defined term's compaction
/// depends on the request's own `@context`, which validation does not hold, and
/// an IRI the client can look up beats a guess it cannot.
fn geo_property_error(attribute_iri: &str, cause: &dyn Display) -> EntityError {
    let name = expanded_to_compact(attribute_iri).unwrap_or(attribute_iri);
    EntityError::validation(format!("GeoProperty '{name}': {cause}"))
}

#[cfg(test)]
mod tests {
    use crate::geo_property_geometry::validate_geo_property_geometries;
    use serde_json::{Value, json};

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const OPERATION_SPACE: &str = "https://uri.etsi.org/ngsi-ld/operationSpace";
    const SITE: &str = "https://example.com/site";
    const GEO_PROPERTY: &str = "https://uri.etsi.org/ngsi-ld/GeoProperty";
    const HAS_VALUE: &str = "https://uri.etsi.org/ngsi-ld/hasValue";

    /// One expanded entity carrying `iri` as a GeoProperty whose literal
    /// `@value` is `geojson`.
    fn entity_with_literal(iri: &str, geojson: Value) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            iri: [{ "@type": [GEO_PROPERTY], HAS_VALUE: [{ "@value": geojson }] }]
        }])
    }

    /// One expanded entity carrying `location` as a GeoProperty in the fully
    /// expanded node form, with the given coordinate member.
    fn entity_with_expanded_node(coordinates: Value) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{
                "@type": [GEO_PROPERTY],
                HAS_VALUE: [{
                    "@type": ["https://purl.org/geojson/vocab#Polygon"],
                    "https://purl.org/geojson/vocab#coordinates": [coordinates]
                }]
            }]
        }])
    }

    /// An expanded coordinate scalar.
    fn ordinate(value: f64) -> Value {
        json!({ "@value": value })
    }

    #[test]
    fn a_two_dimensional_location_is_accepted() {
        let entity = entity_with_literal(LOCATION, json!({"type": "Point", "coordinates": [13.38, 52.52]}));
        assert!(validate_geo_property_geometries(&entity).is_ok());
    }

    #[test]
    fn a_three_dimensional_location_is_accepted() {
        // RFC 7946 section 3.1.1 makes altitude the optional third ordinate,
        // and clause 4.7 adopts RFC 7946 whole, so a height is valid input.
        let entity = entity_with_literal(LOCATION, json!({"type": "Point", "coordinates": [13.38, 52.52, 34.5]}));
        assert!(validate_geo_property_geometries(&entity).is_ok());
    }

    #[test]
    fn a_location_mixing_two_and_three_ordinate_positions_is_accepted() {
        // RFC 7946 nowhere requires a uniform arity within one geometry.
        let entity = entity_with_literal(LOCATION, json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0, 50.0]]}));
        assert!(validate_geo_property_geometries(&entity).is_ok());
    }

    #[test]
    fn a_location_whose_position_carries_four_ordinates_is_rejected() {
        // Clause 5.5.4: an unreadable GeoProperty is BadRequestData.
        let entity = entity_with_literal(LOCATION, json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}));
        let error = validate_geo_property_geometries(&entity).expect_err("four ordinates have no meaning");
        pretty_assertions::assert_eq!(
            error.to_string(),
            "invalid entity operation: GeoProperty 'location': GeoJSON position has 4 ordinates; \
             RFC 7946 allows exactly 2 (longitude, latitude) or 3 (longitude, latitude, altitude)"
        );
    }

    #[test]
    fn a_literal_location_whose_position_carries_one_ordinate_is_rejected() {
        // RFC 7946 section 3.1.1 requires two or more elements. The GeoJSON
        // decoder refuses this one before the domain position type sees it, so
        // the detail is the decoder's rather than the arity check's; what
        // matters for clause 5.5.4 is that the write is refused and the
        // GeoProperty is named.
        let entity = entity_with_literal(LOCATION, json!({"type": "Point", "coordinates": [1.0]}));
        let error = validate_geo_property_geometries(&entity).expect_err("one ordinate is not a position");
        assert!(
            error.to_string().starts_with("invalid entity operation: GeoProperty 'location': "),
            "got {error}"
        );
    }

    #[test]
    fn an_expanded_node_location_whose_position_carries_one_ordinate_is_rejected() {
        let entity = entity_with_expanded_node(json!({"@list": [
            {"@list": [
                {"@list": [ordinate(0.0), ordinate(0.0)]},
                {"@list": [ordinate(4.0)]},
                {"@list": [ordinate(0.0), ordinate(0.0)]}
            ]}
        ]}));
        let error = validate_geo_property_geometries(&entity).expect_err("one ordinate is not a position");
        assert!(error.to_string().contains("1 ordinates"), "got {error}");
    }

    #[test]
    fn a_nested_polygon_vertex_with_four_ordinates_is_rejected() {
        // The check recurses: the offending vertex is in an interior ring, four
        // levels of nesting below the coordinate member.
        let entity = entity_with_literal(
            LOCATION,
            json!({
                "type": "Polygon",
                "coordinates": [
                    [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
                    [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0, 5.0, 9.0], [1.0, 2.0], [1.0, 1.0]]
                ]
            }),
        );
        let error = validate_geo_property_geometries(&entity).expect_err("an interior-ring vertex is still a position");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");
    }

    #[test]
    fn a_custom_geo_property_with_four_ordinates_is_rejected_and_named_by_its_iri() {
        // Clause 4.7.1 lets any attribute be a GeoProperty; the three
        // well-known ones are not a closed list.
        let entity = entity_with_literal(SITE, json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}));
        let error = validate_geo_property_geometries(&entity).expect_err("a user-defined GeoProperty is validated too");
        pretty_assertions::assert_eq!(
            error.to_string(),
            "invalid entity operation: GeoProperty 'https://example.com/site': GeoJSON position has 4 ordinates; \
             RFC 7946 allows exactly 2 (longitude, latitude) or 3 (longitude, latitude, altitude)"
        );
    }

    #[test]
    fn an_operation_space_with_four_ordinates_is_rejected() {
        let entity = entity_with_literal(OPERATION_SPACE, json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}));
        let error = validate_geo_property_geometries(&entity).expect_err("operationSpace is a GeoProperty");
        assert!(error.to_string().contains("operationSpace"), "got {error}");
    }

    #[test]
    fn a_geo_property_written_as_a_json_string_is_validated() {
        // Clause 4.7 permits the GeoJSON document to arrive as a string.
        let entity = entity_with_literal(LOCATION, json!("{\"type\":\"Point\",\"coordinates\":[1.0,2.0,3.0,4.0]}"));
        let error = validate_geo_property_geometries(&entity).expect_err("a stringified document is still a geometry");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");
    }

    #[test]
    fn a_geo_property_in_the_fully_expanded_node_form_is_validated() {
        let entity = entity_with_expanded_node(json!({"@list": [
            {"@list": [
                {"@list": [ordinate(0.0), ordinate(0.0)]},
                {"@list": [ordinate(4.0), ordinate(0.0)]},
                {"@list": [ordinate(4.0), ordinate(4.0), ordinate(1.0), ordinate(9.0)]},
                {"@list": [ordinate(0.0), ordinate(0.0)]}
            ]}
        ]}));
        let error = validate_geo_property_geometries(&entity).expect_err("the expanded node form is validated too");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");
    }

    #[test]
    fn a_valid_geo_property_in_the_fully_expanded_node_form_is_accepted() {
        let entity = entity_with_expanded_node(json!({"@list": [
            {"@list": [
                {"@list": [ordinate(0.0), ordinate(0.0)]},
                {"@list": [ordinate(4.0), ordinate(0.0), ordinate(12.0)]},
                {"@list": [ordinate(0.0), ordinate(0.0)]}
            ]}
        ]}));
        assert!(validate_geo_property_geometries(&entity).is_ok());
    }

    #[test]
    fn an_ordinary_property_carrying_a_coordinate_like_array_is_not_validated_as_a_geometry() {
        // Only instances typed GeoProperty are geometries; a Property whose
        // value happens to be four numbers is untouched.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            "https://example.com/readings": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                HAS_VALUE: [{ "@value": {"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]} }]
            }]
        }]);
        assert!(validate_geo_property_geometries(&entity).is_ok());
    }

    #[test]
    fn every_instance_of_a_multi_instance_geo_property_is_validated() {
        // Clause 4.5.5: the default instance passing is not enough, because a
        // datasetId-tagged instance is read back just as often.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                { "@type": [GEO_PROPERTY], HAS_VALUE: [{ "@value": {"type": "Point", "coordinates": [1.0, 2.0]} }] },
                {
                    "@type": [GEO_PROPERTY],
                    HAS_VALUE: [{ "@value": {"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
                }
            ]
        }]);
        let error = validate_geo_property_geometries(&entity).expect_err("a tagged instance is validated too");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");
    }

    #[test]
    fn a_patch_fragment_given_as_a_bare_object_is_validated() {
        // Merge and partial-update expand to a fragment rather than to a whole
        // entity, and the array wrapper is not guaranteed.
        let entity = json!({
            LOCATION: [{ "@type": [GEO_PROPERTY], HAS_VALUE: [{ "@value": {"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]} }] }]
        });
        let error = validate_geo_property_geometries(&entity).expect_err("a bare fragment is validated");
        assert!(error.to_string().contains("4 ordinates"), "got {error}");
    }

    #[test]
    fn a_location_that_is_a_geometry_collection_is_rejected() {
        // Clause 4.7.1: every RFC 7946 geometry except GeometryCollection.
        let entity = entity_with_literal(
            LOCATION,
            json!({
                "type": "GeometryCollection",
                "geometries": [
                    {"type": "Point", "coordinates": [1.0, 2.0]},
                    {"type": "LineString", "coordinates": [[3.0, 4.0], [5.0, 6.0]]}
                ]
            }),
        );
        let error = validate_geo_property_geometries(&entity).expect_err("a collection is not an allowed GeoProperty geometry");
        pretty_assertions::assert_eq!(
            error.to_string(),
            "invalid entity operation: GeoProperty 'location': GeometryCollection is not an allowed GeoProperty geometry: \
             ETSI GS CIM 009 v1.9.1 clause 4.7.1 allows every RFC 7946 geometry except GeometryCollection"
        );
    }

    #[test]
    fn a_custom_geo_property_that_is_a_geometry_collection_is_rejected_and_named_by_its_iri() {
        let entity = entity_with_literal(SITE, json!({"type": "GeometryCollection", "geometries": []}));
        let error = validate_geo_property_geometries(&entity).expect_err("a user-defined GeoProperty is validated too");
        assert!(
            error
                .to_string()
                .starts_with("invalid entity operation: GeoProperty 'https://example.com/site': GeometryCollection is not an allowed"),
            "got {error}"
        );
    }

    #[test]
    fn a_geometry_collection_in_the_fully_expanded_node_form_is_rejected() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{
                "@type": [GEO_PROPERTY],
                HAS_VALUE: [{
                    "@type": ["https://purl.org/geojson/vocab#GeometryCollection"],
                    "https://purl.org/geojson/vocab#geometries": [
                        {"@list": [{
                            "@type": ["https://purl.org/geojson/vocab#Point"],
                            "https://purl.org/geojson/vocab#coordinates": [{"@list": [ordinate(1.0), ordinate(2.0)]}]
                        }]}
                    ]
                }]
            }]
        }]);
        let error = validate_geo_property_geometries(&entity).expect_err("the expanded node form is validated too");
        assert!(error.to_string().contains("GeometryCollection is not an allowed"), "got {error}");
    }

    #[test]
    fn a_geometry_collection_written_as_a_json_string_is_rejected() {
        // Clause 4.7 permits the GeoJSON document to arrive as a string.
        let entity = entity_with_literal(LOCATION, json!("{\"type\":\"GeometryCollection\",\"geometries\":[]}"));
        let error = validate_geo_property_geometries(&entity).expect_err("a stringified collection is still a collection");
        assert!(error.to_string().contains("GeometryCollection is not an allowed"), "got {error}");
    }

    #[test]
    fn an_entity_carrying_no_geo_property_is_accepted() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        assert!(validate_geo_property_geometries(&entity).is_ok());
    }
}
