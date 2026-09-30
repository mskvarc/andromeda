//! Reading an expanded JSON-LD `GeoJSON` node as the geometry it describes
//! (ETSI GS CIM 009 v1.9.1 clause 4.7, IETF RFC 7946 section 3.1).
//!
//! JSON-LD expansion turns a `GeoProperty` value written as a `GeoJSON` object
//! into a node: `@type` naming the geometry type under the `GeoJSON` vocabulary,
//! and `coordinates` as nested `@list`s of `@value` ordinates. The strict
//! reading refuses a node a write must not store, naming what is wrong and
//! where; the lenient reading, over documents the write path already
//! validated, is the same reading with the refusal dropped.

use andromeda_ngsi_ld_geometry::{
    coordinate_path::CoordinatePath,
    form::{GeometryForm, PolygonRings},
    geojson::GeometryError,
    geometry::Geometry,
    geometry_type::GeometryType,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::{Position, PositionError},
};
use andromeda_ngsi_ld_vocabulary::{expanded::properties, prefix};
use serde_json::Value;
use std::str::FromStr;

/// The `GeoJSON` `type` token of a `GeometryCollection`.
///
/// [`GeometryType`] deliberately does not name it, because ETSI GS CIM 009
/// v1.9.1 clause 4.7.1 allows a `GeoProperty` every RFC 7946 geometry except this
/// one. The token survives only so the write path can recognise a collection in
/// order to refuse it.
const GEOMETRY_COLLECTION_TOKEN: &str = "GeometryCollection";

/// Walk an expanded JSON-LD `GeoJSON` node directly into a [`Geometry`].
///
/// A single-pass walker that emits a [`GeometryForm`] directly: no intermediate
/// `Value` cloning, no string serialisation, no second JSON parser pass. Each
/// position keeps the altitude and the ordinate count it was written with, so
/// what an entity was stored as is what it is read back as.
///
/// Returns `None` for a node that names no geometry type, and for one
/// [`validate_expanded_geo_node`] refuses: this is the lenient read path over
/// documents the write path already validated, so an unusable geometry is
/// skipped rather than reported.
pub fn expanded_geo_node_to_geometry(node: &Value) -> Option<Geometry> {
    decode_typed_geo_node(node).ok().flatten().map(Geometry::new)
}

/// Check that an expanded `GeoJSON` node is a geometry a `GeoProperty` may carry.
///
/// The strict counterpart of [`expanded_geo_node_to_geometry`], for the write
/// path: where the reader skips a geometry it cannot use, this reports why, so
/// a caller can turn it into the `BadRequestData` ETSI GS CIM 009 v1.9.1 clause
/// 5.5.4 requires instead of accepting a `GeoProperty` the broker will later
/// refuse to read back.
///
/// A node typed `GeometryCollection` is refused outright, because clause 4.7.1
/// allows a `GeoProperty` value every geometry IETF RFC 7946 defines except
/// that one, and clause 4.6.3 excludes it from reference geometries as well.
/// A node naming one of the other geometry types is read the way that type
/// nests its `coordinates`, each position built through [`Position::try_new`],
/// each `LineString` through [`LineStringCoordinates::try_new`] and each
/// Polygon ring through [`LinearRing::try_new`]: the rules every other
/// geometry path uses, so the expanded form and the literal form refuse
/// exactly the same geometries. A node naming no geometry type still has every
/// position it carries checked, traversing nesting rather than type, and one
/// carrying no coordinates at all passes -- deciding whether a value is a
/// geometry in the first place is not this function's question.
///
/// # Errors
///
/// Returns [`GeometryError::GeometryCollectionUnsupported`] for a
/// `GeometryCollection`, [`GeometryError::Misnested`] for coordinates nested
/// otherwise than the named type requires, [`GeometryError::LineString`] and
/// [`GeometryError::LinearRing`] for a line or ring RFC 7946 sections 3.1.4
/// and 3.1.6 do not admit, and [`GeometryError::Position`] for the first
/// position whose ordinate count section 3.1.1 gives no meaning to, whose
/// members are not all numbers as that section requires, or whose longitude or
/// latitude lies off the CRS84 axes of section 4.
pub fn validate_expanded_geo_node(node: &Value) -> Result<(), GeometryError> {
    let Some(object) = node.as_object() else {
        return Ok(());
    };
    if is_geometry_collection_node(object.get("@type")) {
        return Err(GeometryError::GeometryCollectionUnsupported);
    }
    if decode_typed_geo_node(node)?.is_some() {
        return Ok(());
    }
    match object.get(properties::COORDINATES.as_str()) {
        Some(coordinates) => validate_positions(expanded_list(coordinates), &mut Vec::new()),
        None => Ok(()),
    }
}

/// Read an expanded node naming one of the geometry types [`GeometryType`]
/// knows as that geometry, strictly.
///
/// `Ok(None)` when the node names no such type or carries no `coordinates`,
/// which leaves the question of what it is to the caller.
fn decode_typed_geo_node(node: &Value) -> Result<Option<GeometryForm>, GeometryError> {
    let Some(object) = node.as_object() else {
        return Ok(None);
    };
    let Some(geometry_type) = object
        .get("@type")
        .and_then(Value::as_array)
        .and_then(|types| types.first())
        .and_then(Value::as_str)
        .and_then(|iri| iri.strip_prefix(prefix::GEOJSON.as_str()))
        .and_then(|token| GeometryType::from_str(token).ok())
    else {
        return Ok(None);
    };
    let Some(coordinates) = object.get(properties::COORDINATES.as_str()) else {
        return Ok(None);
    };
    let reader = CoordinateReader { geometry_type };
    let coordinates = expanded_list(coordinates);
    let enclosing = &mut Vec::new();
    let form = match geometry_type {
        GeometryType::Point => GeometryForm::Point(reader.position(coordinates, enclosing)?),
        GeometryType::MultiPoint => GeometryForm::MultiPoint(reader.positions(coordinates, enclosing)?),
        GeometryType::LineString => GeometryForm::LineString(reader.line_string(coordinates, enclosing)?),
        GeometryType::MultiLineString => GeometryForm::MultiLineString(reader.each(coordinates, enclosing, CoordinateReader::line_string)?),
        GeometryType::Polygon => GeometryForm::Polygon(reader.rings(coordinates, enclosing)?),
        GeometryType::MultiPolygon => GeometryForm::MultiPolygon(reader.each(coordinates, enclosing, CoordinateReader::rings)?),
    };
    Ok(Some(form))
}

/// Reads the expanded `coordinates` of one geometry the way its type nests
/// them (IETF RFC 7946 sections 3.1.2 through 3.1.7).
///
/// Every method takes `enclosing`, the index at each level above the node it
/// reads, and builds a [`CoordinatePath`] from it only to name a refusal, so a
/// geometry that reads cleanly allocates no path at all.
#[derive(Debug, Clone, Copy)]
struct CoordinateReader {
    /// The geometry type whose nesting is read, named in a nesting refusal.
    geometry_type: GeometryType,
}

impl CoordinateReader {
    /// The refusal of a member found where the type nests something else.
    fn misnested(self, enclosing: &[usize]) -> GeometryError {
        GeometryError::Misnested {
            geometry_type: self.geometry_type,
            at: CoordinatePath::new(enclosing.to_vec()),
        }
    }

    /// The items of the list `node` must be.
    fn list<'a>(self, node: &'a Value, enclosing: &[usize]) -> Result<&'a [Value], GeometryError> {
        list_items(node).ok_or_else(|| self.misnested(enclosing))
    }

    /// Read `node` as one position.
    fn position(self, node: &Value, enclosing: &[usize]) -> Result<Position, GeometryError> {
        let ordinates = self.list(node, enclosing)?;
        if ordinates.iter().any(|ordinate| list_items(ordinate).is_some()) {
            return Err(self.misnested(enclosing));
        }
        validate_position(ordinates).map_err(|source| GeometryError::Position {
            at: CoordinatePath::new(enclosing.to_vec()),
            source,
        })
    }

    /// Read `node` as a list of positions.
    fn positions(self, node: &Value, enclosing: &mut Vec<usize>) -> Result<Vec<Position>, GeometryError> {
        self.each(node, enclosing, |reader, item, at| reader.position(item, at))
    }

    /// Read `node` as the positions of one `LineString`.
    fn line_string(self, node: &Value, enclosing: &mut Vec<usize>) -> Result<LineStringCoordinates, GeometryError> {
        LineStringCoordinates::try_new(self.positions(node, enclosing)?).map_err(|source| GeometryError::LineString {
            at: CoordinatePath::new(enclosing.clone()), // clone: the refusal owns the path of the one line it names
            source,
        })
    }

    /// Read `node` as one Polygon ring.
    fn ring(self, node: &Value, enclosing: &mut Vec<usize>) -> Result<LinearRing, GeometryError> {
        LinearRing::try_new(self.positions(node, enclosing)?).map_err(|source| GeometryError::LinearRing {
            at: CoordinatePath::new(enclosing.clone()), // clone: the refusal owns the path of the one ring it names
            source,
        })
    }

    /// Read `node` as the rings of one Polygon, exterior ring first.
    fn rings(self, node: &Value, enclosing: &mut Vec<usize>) -> Result<PolygonRings, GeometryError> {
        self.each(node, enclosing, CoordinateReader::ring).map(PolygonRings::new)
    }

    /// Read every item of the list `node` with `read`, each one level deeper.
    fn each<T>(
        self,
        node: &Value,
        enclosing: &mut Vec<usize>,
        read: impl Fn(CoordinateReader, &Value, &mut Vec<usize>) -> Result<T, GeometryError>,
    ) -> Result<Vec<T>, GeometryError> {
        self.list(node, enclosing)?
            .iter()
            .enumerate()
            .map(|(index, item)| {
                enclosing.push(index);
                let read_item = read(self, item, enclosing);
                enclosing.pop();
                read_item
            })
            .collect()
    }
}

/// The ordered list JSON-LD expansion wraps a `coordinates` member in, as
/// `[{"@list": [...]}]`, so positions are indexed as the compact
/// `coordinates` array a client wrote indexes them.
fn expanded_list(member: &Value) -> &Value {
    match member.as_array().map(Vec::as_slice) {
        Some([list]) if list.get("@list").is_some() => list,
        Some(_) | None => member,
    }
}

/// Whether an expanded `GeoJSON` node's `@type` names a `GeometryCollection`.
fn is_geometry_collection_node(types: Option<&Value>) -> bool {
    types
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|iri| iri.strip_prefix(prefix::GEOJSON.as_str()))
        .any(|token| token == GEOMETRY_COLLECTION_TOKEN)
}

/// Check every position reachable through the coordinate list of a node that
/// names no geometry type, at whatever depth it nests them. `enclosing` holds
/// the index at each level above `node`, so a refusal names the position it
/// refuses.
fn validate_positions(node: &Value, enclosing: &mut Vec<usize>) -> Result<(), GeometryError> {
    let Some(items) = list_items(node) else {
        return Ok(());
    };

    // A list none of whose items is itself a list is a position, whatever its
    // members turn out to be. Deciding this on "every item is a number" instead
    // would let a non-numeric ordinate be mistaken for another level of
    // nesting, and the position would escape the check entirely.
    if !items.is_empty() && items.iter().all(|item| list_items(item).is_none()) {
        return validate_position(items).map(drop).map_err(|source| GeometryError::Position {
            at: CoordinatePath::new(enclosing.clone()), // clone: the refusal owns the path of the one position it names
            source,
        });
    }

    items.iter().enumerate().try_for_each(|(index, item)| {
        enclosing.push(index);
        let checked = validate_positions(item, enclosing);
        enclosing.pop();
        checked
    })
}

/// Read one expanded position under the rules [`Position`] enforces: every
/// member a number, two or three of them, longitude and latitude on the CRS84
/// axes.
fn validate_position(ordinates: &[Value]) -> Result<Position, PositionError> {
    if let Some((index, _member)) = ordinates.iter().enumerate().find(|(_index, item)| scalar_number(item).is_none()) {
        return Err(PositionError::NonNumericOrdinate { index });
    }
    match ordinates {
        [longitude, latitude] => checked_position(longitude, latitude, None),
        [longitude, latitude, altitude] => checked_position(longitude, latitude, Some(altitude)),
        ordinates if ordinates.len() < 2 => Err(PositionError::TooFewOrdinates { ordinates: ordinates.len() }),
        ordinates => Err(PositionError::TooManyOrdinates { ordinates: ordinates.len() }),
    }
}

/// Build one expanded position of two or three ordinates through
/// [`Position::try_new`], the one range rule every geometry path shares.
fn checked_position(longitude: &Value, latitude: &Value, altitude: Option<&Value>) -> Result<Position, PositionError> {
    let ordinate = |index: usize, node: &Value| scalar_number(node).ok_or(PositionError::NonNumericOrdinate { index });
    let altitude = altitude.map(|altitude| ordinate(2, altitude)).transpose()?;
    Position::try_new(ordinate(0, longitude)?, ordinate(1, latitude)?, altitude)
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

#[cfg(test)]
mod tests {
    use crate::expanded::geo_node::{expanded_geo_node_to_geometry, validate_expanded_geo_node};
    use andromeda_ngsi_ld_geometry::{
        coordinate_path::CoordinatePath,
        dimension::GeometryDimension,
        geojson::GeometryError,
        geometry::Geometry,
        geometry_type::GeometryType,
        line_string_coordinates::LineStringError,
        linear_ring::LinearRingError,
        position::PositionError,
    };
    use serde_json::json;

    /// The geometry a compact `GeoJSON` object decodes to through the literal
    /// path, which the node reader must agree with.
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
    fn expanded_geo_node_walker_keeps_the_altitude_of_a_three_ordinate_position() {
        // IETF RFC 7946 section 3.1.1 makes altitude the optional third
        // ordinate, and clause 4.7 adopts RFC 7946 whole, so a stored height
        // must survive the read the index and the matcher share.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}, {"@value": 193.0_f64}]}
            ]
        });
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64, 193.0_f64]});
        let geometry = expanded_geo_node_to_geometry(&expanded).expect("walker decodes an elevated Point");
        pretty_assertions::assert_eq!(geometry, geometry_from_compact(&compact));
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::ThreeD);
    }

    #[test]
    fn expanded_geo_node_walker_preserves_mixed_position_arity_within_one_geometry() {
        // RFC 7946 does not require a uniform arity, so a LineString mixing
        // two- and three-ordinate positions is valid and is kept verbatim.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#LineString"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                    {"@list": [{"@value": 1.0_f64}, {"@value": 1.0_f64}, {"@value": 50.0_f64}]}
                ]}
            ]
        });
        let compact = json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0, 50.0]]});
        let geometry = expanded_geo_node_to_geometry(&expanded).expect("walker decodes a mixed-arity LineString");
        pretty_assertions::assert_eq!(geometry, geometry_from_compact(&compact));
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn expanded_geo_node_walker_returns_none_for_a_geometry_collection() {
        // ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes GeometryCollection from
        // GeoProperty geometries, so nothing readable is ever stored as one and
        // the lenient walker has no geometry to offer.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#GeometryCollection"],
            "https://purl.org/geojson/vocab#geometries": [
                {"@list": [
                    {
                        "@type": ["https://purl.org/geojson/vocab#Point"],
                        "https://purl.org/geojson/vocab#coordinates": [
                            {"@list": [{"@value": 100.0_f64}, {"@value": 0.0_f64}, {"@value": 7.0_f64}]}
                        ]
                    },
                    {
                        "@type": ["https://purl.org/geojson/vocab#LineString"],
                        "https://purl.org/geojson/vocab#coordinates": [
                            {"@list": [
                                {"@list": [{"@value": 101.0_f64}, {"@value": 0.0_f64}]},
                                {"@list": [{"@value": 102.0_f64}, {"@value": 1.0_f64}]}
                            ]}
                        ]
                    }
                ]}
            ]
        });
        assert!(expanded_geo_node_to_geometry(&expanded).is_none());
    }

    #[test]
    fn the_strict_validator_refuses_an_expanded_geometry_collection_node() {
        // Clause 4.7.1: every RFC 7946 geometry except GeometryCollection.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#GeometryCollection"],
            "https://purl.org/geojson/vocab#geometries": [
                {"@list": [
                    {
                        "@type": ["https://purl.org/geojson/vocab#Point"],
                        "https://purl.org/geojson/vocab#coordinates": [
                            {"@list": [{"@value": 100.0_f64}, {"@value": 0.0_f64}]}
                        ]
                    }
                ]}
            ]
        });
        let error = validate_expanded_geo_node(&expanded).expect_err("a collection is not an allowed GeoProperty geometry");
        assert!(matches!(error, GeometryError::GeometryCollectionUnsupported), "got {error:?}");
    }

    #[test]
    fn the_strict_validator_refuses_a_non_numeric_ordinate() {
        // RFC 7946 section 3.1.1 defines a position as an array of numbers, so
        // a stringified ordinate is BadRequestData at the boundary (clause
        // 5.5.4) rather than a payload admitted and found unreadable later.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": "13.4"}, {"@value": 52.52_f64}]}
            ]
        });
        let error = validate_expanded_geo_node(&expanded).expect_err("a string is not an ordinate");
        assert!(
            matches!(
                error,
                GeometryError::Position {
                    source: PositionError::NonNumericOrdinate { index: 0 },
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn the_strict_validator_refuses_a_non_numeric_altitude() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 13.4_f64}, {"@value": 52.52_f64}, {"@value": "high"}]}
            ]
        });
        let error = validate_expanded_geo_node(&expanded).expect_err("an altitude must be a number");
        assert!(
            matches!(
                error,
                GeometryError::Position {
                    source: PositionError::NonNumericOrdinate { index: 2 },
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn the_strict_validator_refuses_a_non_numeric_ordinate_nested_in_a_polygon_ring() {
        // The offending member sits several levels below the coordinate member,
        // so the walk must reach it rather than stop at the first list.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Polygon"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 4.0_f64}, {"@value": "zero"}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]}
                    ]}
                ]}
            ]
        });
        let error = validate_expanded_geo_node(&expanded).expect_err("a ring vertex is still a position");
        assert!(
            matches!(
                error,
                GeometryError::Position {
                    source: PositionError::NonNumericOrdinate { index: 1 },
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    /// An expanded Point at latitude 100, which no CRS84 position has.
    fn expanded_point_past_the_pole() -> serde_json::Value {
        json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": -1.0_f64}, {"@value": 100.0_f64}]}
            ]
        })
    }

    #[test]
    fn the_strict_validator_refuses_an_expanded_position_past_a_pole() {
        // RFC 7946 section 4: the same rule the literal form meets through
        // `Geometry::from_geojson_value`, so neither form of a write escapes it.
        let error = validate_expanded_geo_node(&expanded_point_past_the_pole()).expect_err("latitude 100 is no position");
        assert!(
            matches!(error, GeometryError::Position { ref at, source } if source == PositionError::LatitudeOutOfRange { latitude: 100.0 } && *at == CoordinatePath::default()),
            "got {error:?}"
        );
    }

    #[test]
    fn the_strict_validator_refuses_a_ring_vertex_past_the_antimeridian() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Polygon"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 181.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 1.0_f64}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]}
                    ]}
                ]}
            ]
        });
        let error = validate_expanded_geo_node(&expanded).expect_err("longitude 181 is no position");
        assert!(
            matches!(error, GeometryError::Position { ref at, source } if source == PositionError::LongitudeOutOfRange { longitude: 181.0 } && *at == CoordinatePath::new(vec![0, 1])),
            "got {error:?}"
        );
    }

    /// An expanded position list of `[longitude, latitude]` pairs.
    fn expanded_positions(positions: &[[f64; 2]]) -> serde_json::Value {
        json!({"@list": positions
            .iter()
            .map(|[longitude, latitude]| json!({"@list": [{"@value": longitude}, {"@value": latitude}]}))
            .collect::<Vec<_>>()})
    }

    /// An expanded node of `geometry_type` whose coordinates are `coordinates`.
    fn expanded_node(geometry_type: &str, coordinates: &serde_json::Value) -> serde_json::Value {
        json!({
            "@type": [format!("https://purl.org/geojson/vocab#{geometry_type}")],
            "https://purl.org/geojson/vocab#coordinates": [coordinates]
        })
    }

    #[test]
    fn the_strict_validator_refuses_an_expanded_open_ring_by_its_index() {
        // IETF RFC 7946 section 3.1.6, as the literal form meets it.
        let node = expanded_node(
            "Polygon",
            &json!({"@list": [
                expanded_positions(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]),
                expanded_positions(&[[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]])
            ]}),
        );
        let error = validate_expanded_geo_node(&node).expect_err("the hole is open");
        assert!(
            matches!(error, GeometryError::LinearRing { ref at, source: LinearRingError::Open } if *at == CoordinatePath::new(vec![1])),
            "got {error:?}"
        );
        assert!(
            expanded_geo_node_to_geometry(&node).is_none(),
            "the lenient walker has no geometry for it either"
        );
    }

    #[test]
    fn the_strict_validator_refuses_an_expanded_ring_of_three_positions() {
        let node = expanded_node(
            "MultiPolygon",
            &json!({"@list": [{"@list": [expanded_positions(&[[0.0, 0.0], [1.0, 0.0], [0.0, 0.0]])]}]}),
        );
        let error = validate_expanded_geo_node(&node).expect_err("three positions enclose nothing");
        assert!(
            matches!(
                error,
                GeometryError::LinearRing { ref at, source: LinearRingError::TooFewPositions { positions: 3 } } if *at == CoordinatePath::new(vec![0, 0])
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn the_strict_validator_refuses_an_expanded_line_string_of_one_position() {
        // IETF RFC 7946 section 3.1.4.
        let node = expanded_node("LineString", &expanded_positions(&[[0.0, 0.0]]));
        let error = validate_expanded_geo_node(&node).expect_err("one position traces no line");
        assert!(
            matches!(
                error,
                GeometryError::LineString { ref at, source: LineStringError::TooFewPositions { positions: 1 } } if *at == CoordinatePath::default()
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn the_strict_validator_refuses_a_polygon_nested_one_level_short() {
        // A ring written where the ring list belongs: its positions stand where
        // rings do, and their ordinates where positions do.
        let node = expanded_node("Polygon", &expanded_positions(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]));
        let error = validate_expanded_geo_node(&node).expect_err("the ring list is missing a level");
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
    fn the_strict_validator_accepts_an_expanded_closed_polygon() {
        let node = expanded_node(
            "Polygon",
            &json!({"@list": [expanded_positions(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]])]}),
        );
        assert!(validate_expanded_geo_node(&node).is_ok());
        assert!(expanded_geo_node_to_geometry(&node).is_some());
    }

    #[test]
    fn the_lenient_walker_has_no_geometry_for_a_position_past_a_pole() {
        assert!(expanded_geo_node_to_geometry(&expanded_point_past_the_pole()).is_none());
    }

    #[test]
    fn the_strict_validator_accepts_an_expanded_point_node() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
            ]
        });
        assert!(validate_expanded_geo_node(&expanded).is_ok());
    }

    #[test]
    fn expanded_geo_node_walker_returns_none_for_a_position_with_four_ordinates() {
        // RFC 7946 section 3.1.1 assigns no meaning past the third ordinate.
        // This walker reads documents the write path already validated, so an
        // unusable position skips the geometry instead of raising an error.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [{"@value": 1.0_f64}, {"@value": 2.0_f64}, {"@value": 3.0_f64}, {"@value": 4.0_f64}]}
            ]
        });
        assert!(expanded_geo_node_to_geometry(&expanded).is_none());
    }

    #[test]
    fn expanded_geo_node_walker_returns_none_for_a_position_with_one_ordinate() {
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Point"],
            "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1.0_f64}]}]
        });
        assert!(expanded_geo_node_to_geometry(&expanded).is_none());
    }

    #[test]
    fn a_nested_polygon_vertex_with_four_ordinates_skips_the_whole_geometry() {
        // The walker recurses, so one unusable vertex in an interior ring is
        // enough to refuse the polygon rather than to index a truncated one.
        let expanded = json!({
            "@type": ["https://purl.org/geojson/vocab#Polygon"],
            "https://purl.org/geojson/vocab#coordinates": [
                {"@list": [
                    {"@list": [
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 4.0_f64}, {"@value": 0.0_f64}]},
                        {"@list": [{"@value": 4.0_f64}, {"@value": 4.0_f64}, {"@value": 1.0_f64}, {"@value": 9.0_f64}]},
                        {"@list": [{"@value": 0.0_f64}, {"@value": 0.0_f64}]}
                    ]}
                ]}
            ]
        });
        assert!(expanded_geo_node_to_geometry(&expanded).is_none());
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
}
