//! Decoding GeoProperty values out of an expanded payload (ETSI GS CIM 009 clause 4.7).

use crate::expanded::node::expanded_object_ref;
use andromeda_ngsi_ld_geometry::geometry::{Geometry, GeometryType};
use andromeda_ngsi_ld_vocabulary::{expanded::properties, prefix};
use geo::{Coord, Geometry as GeoGeometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use iri_rs::Iri;
use serde_json::Value;
use std::str::FromStr;

/// Extract the entity's `location` GeoProperty decoded into a [`Geometry`].
pub fn extract_location_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::LOCATION)
}

/// Extract the entity's `observationSpace` GeoProperty decoded into a [`Geometry`].
pub fn extract_observation_space_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::OBSERVATION_SPACE)
}

/// Extract the entity's `operationSpace` GeoProperty decoded into a [`Geometry`].
pub fn extract_operation_space_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::OPERATION_SPACE)
}

/// Decodes the GeoProperty named by `iri` into a [`Geometry`], accepting both
/// the GeoJSON literal form and the fully expanded node form.
fn extract_geo_property_geometry(expanded: &Value, iri: Iri<&'static str>) -> Option<Geometry> {
    let geo_node = geo_node_for_property(expanded, iri)?;
    if let Some(literal) = geo_node.get("@value") {
        return literal_geojson_to_geometry(literal);
    }
    expanded_geo_node_to_geometry(geo_node)
}

/// Look up the inner GeoJSON node for an expanded GeoProperty attribute.
///
/// Returns the "{@type, ...coordinates}" object that lives at
/// `expanded[iri][..].hasValue[0]`, ignoring whether the value was inlined
/// as a literal `@value` or expanded as a nested JSON-LD GeoJSON node —
/// callers branch on the presence of `@value` to pick the decoding path.
fn geo_node_for_property<'a>(expanded: &'a Value, iri: Iri<&'static str>) -> Option<&'a Value> {
    let obj = expanded_object_ref(expanded)?;
    let prop = obj.get(iri.as_str())?;
    prop.as_array()
        .and_then(|arr| arr.last())
        .and_then(|inst| inst.get(properties::VALUE.as_str()))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
}

/// Decode a literal-GeoJSON `@value` (string or already-compact object) into a [`Geometry`].
fn literal_geojson_to_geometry(literal: &Value) -> Option<Geometry> {
    match literal {
        Value::String(s) => {
            let value: Value = serde_json::from_str(s).ok()?;
            Geometry::from_geojson_value(&value).ok()
        }
        Value::Object(_) => Geometry::from_geojson_value(literal).ok(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) => None,
    }
}

/// Walk an expanded JSON-LD GeoJSON node directly into a [`Geometry`].
///
/// A single-pass walker that emits `geo::Geometry<f64>` directly: no
/// intermediate `Value` cloning, no string serialisation, no second JSON
/// parser pass. Returns `None` for unknown geometry types or malformed input so
/// the caller can fall through to the slow path if needed.
pub fn expanded_geo_node_to_geometry(node: &Value) -> Option<Geometry> {
    walk_expanded_geo_node(node).map(Geometry::from)
}

/// Reads an expanded GeoJSON node, dispatching on its `@type` to the walker
/// for that geometry kind.
fn walk_expanded_geo_node(node: &Value) -> Option<GeoGeometry<f64>> {
    let obj = node.as_object()?;
    let token = obj.get("@type")?.as_array()?.first()?.as_str()?.strip_prefix(prefix::GEOJSON.as_str())?;
    let geom_type = GeometryType::from_str(token).ok()?;

    let coords_outer = obj.get(properties::COORDINATES.as_str())?.as_array()?.first()?;

    match geom_type {
        GeometryType::Point => Some(GeoGeometry::Point(Point::from(walk_position(coords_outer)?))),
        GeometryType::MultiPoint => {
            let points: Vec<Point<f64>> = walk_position_list(coords_outer)?.into_iter().map(Point::from).collect();
            Some(GeoGeometry::MultiPoint(MultiPoint::new(points)))
        }
        GeometryType::LineString => Some(GeoGeometry::LineString(LineString::from(walk_position_list(coords_outer)?))),
        GeometryType::MultiLineString => {
            let lines = walk_linestring_list(coords_outer)?;
            Some(GeoGeometry::MultiLineString(MultiLineString::new(lines)))
        }
        GeometryType::Polygon => Some(GeoGeometry::Polygon(walk_polygon(coords_outer)?)),
        GeometryType::MultiPolygon => {
            let polys = walk_polygon_list(coords_outer)?;
            Some(GeoGeometry::MultiPolygon(MultiPolygon::new(polys)))
        }
    }
}

/// Iterate the children of either a bare `Array` or an `{"@list": [...]}`
/// wrapper — the two shapes JSON-LD expansion produces for ordered lists.
fn list_items(node: &Value) -> Option<&[Value]> {
    if let Some(arr) = node.as_array() {
        return Some(arr.as_slice());
    }
    node.as_object().and_then(|o| o.get("@list")).and_then(|v| v.as_array()).map(Vec::as_slice)
}

/// Read a numeric leaf from either a bare JSON number or `{"@value": n}`.
fn scalar_number(node: &Value) -> Option<f64> {
    if let Some(n) = node.as_f64() {
        return Some(n);
    }
    node.as_object().and_then(|o| o.get("@value")).and_then(Value::as_f64)
}

/// Reads one expanded coordinate pair, discarding any third (altitude)
/// ordinate, which the `geo` crate's 2D model cannot carry.
fn walk_position(node: &Value) -> Option<Coord<f64>> {
    let items = list_items(node)?;
    if items.len() < 2 {
        return None;
    }
    let x = scalar_number(&items[0])?;
    let y = scalar_number(&items[1])?;
    Some(Coord { x, y })
}

/// Reads an expanded list of coordinate pairs, failing on the first pair that
/// does not parse.
fn walk_position_list(node: &Value) -> Option<Vec<Coord<f64>>> {
    let items = list_items(node)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(walk_position(item)?);
    }
    Some(out)
}

/// Reads an expanded list of coordinate-pair lists as line strings.
fn walk_linestring_list(node: &Value) -> Option<Vec<LineString<f64>>> {
    let items = list_items(node)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(LineString::from(walk_position_list(item)?));
    }
    Some(out)
}

/// Reads an expanded ring list as a polygon: the first ring is the exterior,
/// every subsequent ring an interior (RFC 7946 clause 3.1.6).
fn walk_polygon(node: &Value) -> Option<Polygon<f64>> {
    let rings = list_items(node)?;
    if rings.is_empty() {
        return None;
    }
    let exterior = LineString::from(walk_position_list(&rings[0])?);
    let mut interiors = Vec::with_capacity(rings.len().saturating_sub(1));
    for ring in &rings[1..] {
        interiors.push(LineString::from(walk_position_list(ring)?));
    }
    Some(Polygon::new(exterior, interiors))
}

/// Reads an expanded list of ring lists as polygons.
fn walk_polygon_list(node: &Value) -> Option<Vec<Polygon<f64>>> {
    let items = list_items(node)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(walk_polygon(item)?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use crate::expanded::geo_property::{expanded_geo_node_to_geometry, extract_location_geometry};
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use serde_json::json;

    /// Build the expected [`Geometry`] for a known compact-GeoJSON object by
    /// routing through the `from_geojson_value` path; the direct walker must
    /// produce an equal geometry.
    fn geometry_from_compact(compact: &serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(compact).expect("compact geojson decodes")
    }

    #[test]
    fn expanded_geo_node_walker_decodes_point_identically_to_the_geojson_path() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
            ]
        });
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64]});
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_geometry(&expanded).expect("walker decodes Point"),
            geometry_from_compact(&compact)
        );
    }

    #[test]
    fn expanded_geo_node_walker_decodes_linestring() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#LineString"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                    {"@list": [{"@value": 1.0_f64}, {"@value": 2.0_f64}]},
                    {"@list": [{"@value": 3.0_f64}, {"@value": 4.0_f64}]}
                ]}
            ]
        });
        let compact = json!({
            "type": "LineString",
            "coordinates": [[0.0, 0.0], [1.0, 2.0], [3.0, 4.0]]
        });
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_geometry(&expanded).expect("walker decodes LineString"),
            geometry_from_compact(&compact)
        );
    }

    #[test]
    fn expanded_geo_node_walker_decodes_polygon_with_hole() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Polygon"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 4.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 4.0_f64}, {"@value": 4.0_f64}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 4.0_f64}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]}
                    ]},
                    {"@list": [
                        {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]},
                        {"@list": [{"@value": 2.0_f64}, {"@value": 1.0_f64}]},
                        {"@list": [{"@value": 2.0_f64}, {"@value": 2.0_f64}]},
                        {"@list": [{"@value": 1.0_f64}, {"@value": 2.0_f64}]},
                        {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]}
                    ]}
                ]}
            ]
        });
        let compact = json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
                [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0], [1.0, 1.0]]
            ]
        });
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_geometry(&expanded).expect("walker decodes Polygon"),
            geometry_from_compact(&compact)
        );
    }

    #[test]
    fn expanded_geo_node_walker_decodes_multipoint_and_multilinestring() {
        let multipoint_expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#MultiPoint"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                    {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]}
                ]}
            ]
        });
        let multipoint_compact = json!({"type": "MultiPoint", "coordinates": [[0.0, 0.0], [1.0, 1.0]]});
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_geometry(&multipoint_expanded).expect("MultiPoint"),
            geometry_from_compact(&multipoint_compact)
        );

        let multilinestring_expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#MultiLineString"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}]}
                    ]},
                    {"@list": [
                        {"@list": [{"@value": 2.0_f64}, {"@value": 2.0_f64}]},
                        {"@list": [{"@value": 3.0_f64}, {"@value": 3.0_f64}]}
                    ]}
                ]}
            ]
        });
        let multilinestring_compact = json!({
            "type": "MultiLineString",
            "coordinates": [[[0.0, 0.0], [1.0, 1.0]], [[2.0, 2.0], [3.0, 3.0]]]
        });
        pretty_assertions::assert_eq!(
            expanded_geo_node_to_geometry(&multilinestring_expanded).expect("MultiLineString"),
            geometry_from_compact(&multilinestring_compact)
        );
    }

    #[test]
    fn expanded_geo_node_walker_returns_none_for_unknown_type() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#NotAGeometry"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]}
            ]
        });
        assert!(expanded_geo_node_to_geometry(&expanded).is_none());
    }

    #[test]
    fn extract_geo_property_wkb_uses_direct_walker_for_expanded_form() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://uri.etsi.org/ngsi-ld/location": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [
                        {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
                    ]
                }]
            }]
        }]);
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64]});
        pretty_assertions::assert_eq!(extract_location_geometry(&entity).expect("location WKB"), geometry_from_compact(&compact));
    }
}
