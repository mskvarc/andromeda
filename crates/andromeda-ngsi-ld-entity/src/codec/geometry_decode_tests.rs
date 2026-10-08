//! The strict geometry reading of a `GeoProperty` value (clause 4.7, IETF RFC
//! 7946 section 3.1).

use crate::{entity_state::EntityState, geo_value::GeoValue, refusal::EntityRefusal};
use andromeda_ngsi_ld_geometry::{
    coordinate_path::CoordinatePath,
    dimension::GeometryDimension,
    form::GeometryForm,
    geojson::GeometryError,
    geometry_type::GeometryType,
    line_string_coordinates::LineStringError,
    linear_ring::LinearRingError,
    position::PositionError,
};
use serde_json::{Value, json};

/// Decode an entity whose `location` holds the expanded geometry node `node`.
fn decode(node: &Value) -> Result<GeometryForm, EntityRefusal> {
    let document = json!([{
        "@id": "urn:ngsi-ld:Building:1",
        "@type": ["https://ex.org/Building"],
        "https://uri.etsi.org/ngsi-ld/location": [{"@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"], "https://uri.etsi.org/ngsi-ld/hasValue": [node]}]
    }]);
    let state = EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON"))?;
    let form = state.location_geometry().cloned();
    Ok(form.expect("a decoded location"))
}

/// The geometry fault a refusal carries.
fn geometry_error(node: &Value) -> GeometryError {
    match decode(node) {
        Err(EntityRefusal::GeoProperty { source, .. }) => source,
        other => panic!("expected a geometry refusal, got {other:?}"),
    }
}

/// An expanded position list of `[longitude, latitude]` pairs.
fn expanded_positions(positions: &[[f64; 2]]) -> Value {
    json!({"@list": positions.iter().map(|[longitude, latitude]| json!({"@list": [{"@value": longitude}, {"@value": latitude}]})).collect::<Vec<_>>()})
}

/// An expanded node of `geometry_type` whose coordinates are `coordinates`.
fn expanded_node(geometry_type: &str, coordinates: &Value) -> Value {
    json!({"@type": [format!("https://purl.org/geojson/vocab#{geometry_type}")], "https://purl.org/geojson/vocab#coordinates": [coordinates]})
}

/// The geometry a compact `GeoJSON` object reads as.
fn compact(value: &Value) -> GeometryForm {
    GeometryForm::from_compact_text(&value.to_string()).expect("compact GeoJSON")
}

#[test]
fn every_geometry_type_decodes_identically_to_the_compact_reading() {
    let cases = [
        (
            expanded_node("Point", &json!({"@list": [{"@value": 16.37}, {"@value": 48.2}]})),
            json!({"type": "Point", "coordinates": [16.37, 48.2]}),
        ),
        (
            expanded_node("LineString", &expanded_positions(&[[0.0, 0.0], [1.0, 1.0]])),
            json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
        ),
        (
            expanded_node(
                "Polygon",
                &json!({"@list": [expanded_positions(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]), expanded_positions(&[[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 1.0]])]}),
            ),
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]], [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 1.0]]]}),
        ),
        (
            expanded_node("MultiPoint", &expanded_positions(&[[0.0, 0.0], [1.0, 1.0]])),
            json!({"type": "MultiPoint", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
        ),
        (
            expanded_node(
                "MultiLineString",
                &json!({"@list": [expanded_positions(&[[0.0, 0.0], [1.0, 1.0]]), expanded_positions(&[[2.0, 2.0], [3.0, 3.0]])]}),
            ),
            json!({"type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], [[2.0, 2.0], [3.0, 3.0]]]}),
        ),
    ];
    for (expanded, compact_value) in cases {
        pretty_assertions::assert_eq!(decode(&expanded).expect("a geometry"), compact(&compact_value), "{compact_value}");
    }
}

/// RFC 7946 section 3.1.1 makes altitude the optional third ordinate.
#[test]
fn the_altitude_of_a_three_ordinate_position_is_kept() {
    let form = decode(&expanded_node(
        "Point",
        &json!({"@list": [{"@value": 16.37}, {"@value": 48.2}, {"@value": 193}]}),
    ))
    .expect("an elevated point");
    pretty_assertions::assert_eq!(form, compact(&json!({"type": "Point", "coordinates": [16.37, 48.2, 193.0]})));
    pretty_assertions::assert_eq!(form.dimension(), GeometryDimension::ThreeD);
}

#[test]
fn mixed_position_arity_within_one_geometry_is_refused_at_the_disagreeing_position() {
    let node = expanded_node(
        "LineString",
        &json!({"@list": [{"@list": [{"@value": 0}, {"@value": 0}]}, {"@list": [{"@value": 1}, {"@value": 1}, {"@value": 50}]}]}),
    );
    let error = geometry_error(&node);
    assert!(
        matches!(error, GeometryError::MixedArity { ref at } if *at == CoordinatePath::new(vec![1])),
        "got {error:?}"
    );
}

/// Clause 4.7.1: every RFC 7946 geometry except `GeometryCollection`.
#[test]
fn an_expanded_geometry_collection_node_is_refused() {
    let node = json!({
        "@type": ["https://purl.org/geojson/vocab#GeometryCollection"],
        "https://purl.org/geojson/vocab#geometries": [{"@list": [expanded_node("Point", &json!({"@list": [{"@value": 100}, {"@value": 0}]}))]}]
    });
    assert!(matches!(geometry_error(&node), GeometryError::GeometryCollectionUnsupported));
}

#[test]
fn a_non_numeric_ordinate_is_refused_with_its_index() {
    let cases = [
        (expanded_node("Point", &json!({"@list": [{"@value": "13.4"}, {"@value": 52.52}]})), 0),
        (
            expanded_node("Point", &json!({"@list": [{"@value": 13.4}, {"@value": 52.52}, {"@value": "high"}]})),
            2,
        ),
        (
            expanded_node(
                "Polygon",
                &json!({"@list": [{"@list": [{"@list": [{"@value": 0}, {"@value": 0}]}, {"@list": [{"@value": 4}, {"@value": "zero"}]}, {"@list": [{"@value": 0}, {"@value": 0}]}]}]}),
            ),
            1,
        ),
    ];
    for (node, expected) in cases {
        let error = geometry_error(&node);
        assert!(
            matches!(error, GeometryError::Position { source: PositionError::NonNumericOrdinate { index }, .. } if index == expected),
            "got {error:?}"
        );
    }
}

/// RFC 7946 section 4, as the literal form meets it.
#[test]
fn an_expanded_position_past_a_pole_is_refused() {
    let error = geometry_error(&expanded_node("Point", &json!({"@list": [{"@value": -1}, {"@value": 100}]})));
    assert!(
        matches!(error, GeometryError::Position { ref at, source } if source == PositionError::LatitudeOutOfRange { latitude: 100.0 } && *at == CoordinatePath::default()),
        "got {error:?}"
    );
}

#[test]
fn a_ring_vertex_past_the_antimeridian_is_refused_with_its_path() {
    let node = expanded_node(
        "Polygon",
        &json!({"@list": [expanded_positions(&[[0.0, 0.0], [181.0, 0.0], [0.0, 1.0], [0.0, 0.0]])]}),
    );
    let error = geometry_error(&node);
    assert!(
        matches!(error, GeometryError::Position { ref at, source } if source == PositionError::LongitudeOutOfRange { longitude: 181.0 } && *at == CoordinatePath::new(vec![0, 1])),
        "got {error:?}"
    );
}

/// IETF RFC 7946 section 3.1.6.
#[test]
fn an_expanded_open_ring_is_refused_by_its_index() {
    let node = expanded_node(
        "Polygon",
        &json!({"@list": [expanded_positions(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]), expanded_positions(&[[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]])]}),
    );
    let error = geometry_error(&node);
    assert!(
        matches!(error, GeometryError::LinearRing { ref at, source: LinearRingError::Open } if *at == CoordinatePath::new(vec![1])),
        "got {error:?}"
    );
}

#[test]
fn an_expanded_ring_of_three_positions_is_refused() {
    let node = expanded_node(
        "MultiPolygon",
        &json!({"@list": [{"@list": [expanded_positions(&[[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]])]}]}),
    );
    let error = geometry_error(&node);
    assert!(
        matches!(error, GeometryError::LinearRing { ref at, source: LinearRingError::TooFewPositions { positions: 3 } } if *at == CoordinatePath::new(vec![0, 0])),
        "got {error:?}"
    );
}

/// IETF RFC 7946 section 3.1.4.
#[test]
fn an_expanded_line_string_of_one_position_is_refused() {
    let error = geometry_error(&expanded_node("LineString", &expanded_positions(&[[0.0, 0.0]])));
    assert!(
        matches!(error, GeometryError::LineString { ref at, source: LineStringError::TooFewPositions { positions: 1 } } if *at == CoordinatePath::default()),
        "got {error:?}"
    );
}

/// A ring written where the ring list belongs.
#[test]
fn a_polygon_nested_one_level_short_is_refused() {
    let error = geometry_error(&expanded_node(
        "Polygon",
        &expanded_positions(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]),
    ));
    assert!(
        matches!(error, GeometryError::Misnested { geometry_type: GeometryType::Polygon, ref at } if *at == CoordinatePath::new(vec![0, 0])),
        "got {error:?}"
    );
    pretty_assertions::assert_eq!(
        error.to_string(),
        "GeoJSON coordinates do not nest as a Polygon requires (at coordinates[0][0])"
    );
}

#[test]
fn a_position_of_four_or_one_ordinates_is_refused() {
    let four = geometry_error(&expanded_node(
        "Point",
        &json!({"@list": [{"@value": 1}, {"@value": 2}, {"@value": 3}, {"@value": 4}]}),
    ));
    assert!(
        matches!(
            four,
            GeometryError::Position {
                source: PositionError::TooManyOrdinates { ordinates: 4 },
                ..
            }
        ),
        "got {four:?}"
    );
    let one = geometry_error(&expanded_node("Point", &json!({"@list": [{"@value": 1}]})));
    assert!(
        matches!(
            one,
            GeometryError::Position {
                source: PositionError::TooFewOrdinates { ordinates: 1 },
                ..
            }
        ),
        "got {one:?}"
    );
}

#[test]
fn a_nested_polygon_vertex_with_four_ordinates_refuses_the_whole_geometry() {
    let node = expanded_node(
        "Polygon",
        &json!({"@list": [{"@list": [
            {"@list": [{"@value": 0}, {"@value": 0}]},
            {"@list": [{"@value": 4}, {"@value": 0}]},
            {"@list": [{"@value": 4}, {"@value": 4}, {"@value": 1}, {"@value": 9}]},
            {"@list": [{"@value": 0}, {"@value": 0}]}
        ]}]}),
    );
    assert!(matches!(geometry_error(&node), GeometryError::Position { .. }));
}

#[test]
fn an_unknown_geometry_type_is_refused_without_echoing_it() {
    let error = geometry_error(&expanded_node("NotAGeometry<script>", &json!({"@list": [{"@value": 0}, {"@value": 0}]})));
    assert!(matches!(error, GeometryError::UnknownType(_)), "got {error:?}");
    assert!(!error.to_string().contains("<script>"));
}

#[test]
fn a_typed_node_without_coordinates_is_refused() {
    let error = geometry_error(&json!({"@type": ["https://purl.org/geojson/vocab#Point"]}));
    assert!(matches!(error, GeometryError::MissingCoordinates), "got {error:?}");
}

#[test]
fn a_string_geometry_reads_as_its_geometry_and_the_null_as_a_deletion() {
    let document = |value: &str| {
        json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://ex.org/Building"],
            "https://uri.etsi.org/ngsi-ld/location": [{"@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}]
        }])
    };
    let state = EntityState::from_slice(&serde_json::to_vec(&document("{\"type\":\"Point\",\"coordinates\":[13.4,52.5]}")).expect("JSON")).expect("a state");
    let encoded = state
        .attributes()
        .get("https://uri.etsi.org/ngsi-ld/location")
        .and_then(|instances| instances.get(None))
        .and_then(|instance| instance.value().geometry());
    assert!(matches!(encoded, Some(GeoValue::Encoded(_))));
    let refusal = EntityState::from_slice(&serde_json::to_vec(&document("urn:ngsi-ld:null")).expect("JSON")).expect_err("state holds no deletion");
    assert!(matches!(refusal, EntityRefusal::ReservedNullValue { .. }), "{refusal:?}");
}
