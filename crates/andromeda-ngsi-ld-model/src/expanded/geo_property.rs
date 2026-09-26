//! Decoding `GeoProperty` values out of an expanded payload (ETSI GS CIM 009 clause 4.7).

use crate::expanded::{attribute_expiry::instance_expires_at, dataset_id::instance_has_dataset_id, node::expanded_object_ref};
use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    geojson::GeometryError,
    geometry::Geometry,
    geometry_type::GeometryType,
    position::{Position, PositionError},
};
use andromeda_ngsi_ld_vocabulary::{expanded::properties, prefix};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::str::FromStr;

/// The `GeoJSON` `type` token of a `GeometryCollection`.
///
/// [`GeometryType`] deliberately does not name it, because ETSI GS CIM 009
/// v1.9.1 clause 4.7.1 allows a `GeoProperty` every RFC 7946 geometry except this
/// one. The token survives only so the write path can recognise a collection in
/// order to refuse it.
const GEOMETRY_COLLECTION_TOKEN: &str = "GeometryCollection";

/// Extract the entity's `location` `GeoProperty` decoded into a [`Geometry`].
#[must_use]
pub fn extract_location_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::LOCATION.as_str())
}

/// The instant after which every `location` instance of the entity has
/// expired, or `None` when at least one instance never expires (clause 4.22).
///
/// A geo query against the default `GeoProperty` must not match an entity whose
/// every `location` instance is past its `expiresAt`, and it reads the answer
/// from a stored column rather than the payload. The column carries the latest
/// instance expiry, so "some instance still stands" is `column IS NULL OR
/// column > now()`: NULL when any instance carries no expiry, otherwise the
/// maximum. An entity without a `location` yields `None` as well; the geometry
/// column being NULL already excludes it.
pub fn extract_location_expiry(expanded: &Value) -> Option<DateTime<Utc>> {
    let instances = geo_property_instances(expanded, properties::LOCATION.as_str())?;
    instances
        .iter()
        .map(instance_expires_at)
        .try_fold(None, |latest: Option<DateTime<Utc>>, expires_at| {
            let expires_at = expires_at?;
            Some(Some(latest.map_or(expires_at, |latest| latest.max(expires_at))))
        })?
}

/// Extract the entity's `observationSpace` `GeoProperty` decoded into a [`Geometry`].
#[must_use]
pub fn extract_observation_space_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::OBSERVATION_SPACE.as_str())
}

/// Extract the entity's `operationSpace` `GeoProperty` decoded into a [`Geometry`].
#[must_use]
pub fn extract_operation_space_geometry(expanded: &Value) -> Option<Geometry> {
    extract_geo_property_geometry(expanded, properties::OPERATION_SPACE.as_str())
}

/// Decode the default instance of the `GeoProperty` named by `iri` into a
/// [`Geometry`], accepting both the `GeoJSON` literal form and the fully expanded
/// node form.
///
/// One geometry per entity, chosen by [`default_instance`] rather than by array
/// order. This is what the spatial index is built from: an R-tree partition
/// holds one bounding rectangle per entity, so a multi-instance `GeoProperty` has
/// to be reduced to a single geometry before it can be indexed, and clause
/// 4.5.16.1's default instance is the only defensible choice of which.
///
/// Matching is a different question and does not use this reader: clause 4.10
/// counts an entity as matching when *any* instance meets the restriction, for
/// which [`geo_property_instance_geometries`] yields all of them. The index
/// therefore over- and under-approximates a multi-instance `GeoProperty`, and
/// every consumer of it re-checks the exact relation afterwards — the spatial
/// prefilter falls back to the store, and the notification path re-evaluates
/// through the residual filter.
#[must_use]
pub fn extract_geo_property_geometry(expanded: &Value, iri: &str) -> Option<Geometry> {
    let instances = geo_property_instances(expanded, iri)?;
    geometry_of_instance(default_instance(instances)?)
}

/// Decode every instance of the `GeoProperty` named by `iri`, in payload order.
///
/// Clause 4.10: where a `GeoProperty` has several `datasetId` instances, one of
/// them meeting a geo restriction is enough, so a matcher reads all of them
/// rather than the default one. Instances that carry no decodable geometry are
/// skipped, exactly as the SQL rendering skips `current_attribute_values` rows whose
/// `value_geometry` is null.
pub fn geo_property_instance_geometries<'a>(expanded: &'a Value, iri: &str) -> impl Iterator<Item = Geometry> + 'a {
    geo_property_instances(expanded, iri)
        .unwrap_or_default()
        .iter()
        .filter_map(geometry_of_instance)
}

/// The instance array of one expanded attribute, if the entity conveys it.
fn geo_property_instances<'a>(expanded: &'a Value, iri: &str) -> Option<&'a [Value]> {
    expanded_object_ref(expanded)?.get(iri)?.as_array().map(Vec::as_slice)
}

/// Decode one attribute instance's value into a [`Geometry`].
///
/// Reads the "{@type, ...coordinates}" object that lives at
/// `<instance>.hasValue[0]`, ignoring whether the value was inlined as a
/// literal `@value` or expanded as a nested JSON-LD `GeoJSON` node.
fn geometry_of_instance(instance: &Value) -> Option<Geometry> {
    let geo_node = instance
        .get(properties::VALUE.as_str())
        .and_then(Value::as_array)
        .and_then(|values| values.first())?;
    if let Some(literal) = geo_node.get("@value") {
        return literal_geojson_to_geometry(literal);
    }
    expanded_geo_node_to_geometry(geo_node)
}

/// Select the default instance of a multi-attribute (clause 4.5.16.1): the one
/// carrying no `datasetId`, falling back to the first when all are tagged.
///
/// Selection is deliberate rather than an accident of `first()` versus
/// `last()`, so two readers of the same payload cannot disagree.
fn default_instance(instances: &[Value]) -> Option<&Value> {
    instances
        .iter()
        .find(|instance| !instance_has_dataset_id(instance))
        .or_else(|| instances.first())
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

/// Walk an expanded JSON-LD `GeoJSON` node directly into a [`Geometry`].
///
/// A single-pass walker that emits a [`GeometryForm`] directly: no intermediate
/// `Value` cloning, no string serialisation, no second JSON parser pass. Each
/// position keeps the altitude and the ordinate count it was written with, so
/// what an entity was stored as is what it is read back as.
///
/// Returns `None` for unknown geometry types or malformed input so the caller
/// can fall through to the slow path if needed.
pub fn expanded_geo_node_to_geometry(node: &Value) -> Option<Geometry> {
    walk_expanded_geo_node(node).map(Geometry::new)
}

/// Reads an expanded `GeoJSON` node, dispatching on its `@type` to the walker
/// for that geometry kind.
///
/// A `GeometryCollection` yields `None`: clause 4.7.1 excludes it from `GeoProperty`
/// geometries, so nothing the write path accepted can be one, and the lenient
/// read simply has no geometry to offer for it.
fn walk_expanded_geo_node(node: &Value) -> Option<GeometryForm> {
    let obj = node.as_object()?;
    let token = obj.get("@type")?.as_array()?.first()?.as_str()?.strip_prefix(prefix::GEOJSON.as_str())?;

    let coordinates = obj.get(properties::COORDINATES.as_str())?.as_array()?.first()?;

    match GeometryType::from_str(token).ok()? {
        GeometryType::Point => Some(GeometryForm::Point(walk_position(coordinates)?)),
        GeometryType::MultiPoint => Some(GeometryForm::MultiPoint(walk_positions(coordinates)?)),
        GeometryType::LineString => Some(GeometryForm::LineString(walk_positions(coordinates)?)),
        GeometryType::MultiLineString => Some(GeometryForm::MultiLineString(walk_position_lists(coordinates)?)),
        GeometryType::Polygon => Some(GeometryForm::Polygon(walk_rings(coordinates)?)),
        GeometryType::MultiPolygon => Some(GeometryForm::MultiPolygon(walk_ring_lists(coordinates)?)),
    }
}

/// Check that an expanded `GeoJSON` node is a geometry a `GeoProperty` may carry.
///
/// The strict counterpart of [`expanded_geo_node_to_geometry`], for the write
/// path: where the reader skips a geometry it cannot use, this reports why, so
/// a caller can turn it into the `BadRequestData` ETSI GS CIM 009 v1.9.1 clause
/// 5.5.4 requires instead of accepting a `GeoProperty` the broker will later
/// refuse to read back.
///
/// Two things are checked. A node typed `GeometryCollection` is refused outright,
/// because clause 4.7.1 allows a `GeoProperty` value every geometry IETF RFC 7946
/// defines except that one, and clause 4.6.3 excludes it from reference
/// geometries as well. Otherwise the ordinate count of every position is
/// checked, traversing nesting rather than geometry type: a position is the
/// innermost list, whatever encloses it, so one walk covers Point through
/// `MultiPolygon`. A node carrying no coordinates at all passes — deciding
/// whether a value is a geometry in the first place is not this function's
/// question.
///
/// # Errors
///
/// Returns [`GeometryError::GeometryCollectionUnsupported`] for a
/// `GeometryCollection`, and [`GeometryError::Position`] for the first position
/// whose ordinate count RFC 7946 section 3.1.1 gives no meaning to, or whose
/// members are not all numbers as that section requires.
pub fn validate_expanded_geo_node(node: &Value) -> Result<(), GeometryError> {
    let Some(object) = node.as_object() else {
        return Ok(());
    };

    if is_geometry_collection_node(object.get("@type")) {
        return Err(GeometryError::GeometryCollectionUnsupported);
    }

    match object.get(properties::COORDINATES.as_str()) {
        Some(coordinates) => Ok(validate_coordinate_arity(coordinates)?),
        None => Ok(()),
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

/// Check the ordinate count of every position reachable through one expanded
/// coordinate member, at whatever depth its geometry type nests them.
fn validate_coordinate_arity(node: &Value) -> Result<(), PositionError> {
    let Some(items) = list_items(node) else {
        return Ok(());
    };

    // A list none of whose items is itself a list is a position, whatever its
    // members turn out to be. Deciding this on "every item is a number" instead
    // would let a non-numeric ordinate be mistaken for another level of
    // nesting, and the position would escape the check entirely.
    if !items.is_empty() && items.iter().all(|item| list_items(item).is_none()) {
        if let Some((index, _member)) = items.iter().enumerate().find(|(_index, item)| scalar_number(item).is_none()) {
            return Err(PositionError::NonNumericOrdinate { index });
        }
        return match items.len() {
            2 | 3 => Ok(()),
            ordinates if ordinates < 2 => Err(PositionError::TooFewOrdinates { ordinates }),
            ordinates => Err(PositionError::TooManyOrdinates { ordinates }),
        };
    }

    items.iter().try_for_each(validate_coordinate_arity)
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

/// Reads one expanded position, keeping the optional third ordinate RFC 7946
/// section 3.1.1 defines as altitude.
///
/// A position with any other ordinate count has no meaning under that section
/// and yields `None`. This is the lenient read path over documents the write
/// path already validated, so an unusable position skips the whole geometry
/// rather than reporting a failure.
fn walk_position(node: &Value) -> Option<Position> {
    match list_items(node)? {
        [longitude, latitude] => Some(Position::new(scalar_number(longitude)?, scalar_number(latitude)?, None)),
        [longitude, latitude, altitude] => Some(Position::new(
            scalar_number(longitude)?,
            scalar_number(latitude)?,
            Some(scalar_number(altitude)?),
        )),
        _other => None,
    }
}

/// Reads an expanded list of positions, failing on the first that does not parse.
fn walk_positions(node: &Value) -> Option<Vec<Position>> {
    list_items(node)?.iter().map(walk_position).collect()
}

/// Reads an expanded list of position lists, as a `MultiLineString` carries.
fn walk_position_lists(node: &Value) -> Option<Vec<Vec<Position>>> {
    list_items(node)?.iter().map(walk_positions).collect()
}

/// Reads an expanded ring list as a polygon: the first ring is the exterior,
/// every subsequent ring an interior (IETF RFC 7946 section 3.1.6).
fn walk_rings(node: &Value) -> Option<PolygonRings> {
    walk_position_lists(node).map(PolygonRings::new)
}

/// Reads an expanded list of ring lists, as a `MultiPolygon` carries.
fn walk_ring_lists(node: &Value) -> Option<Vec<PolygonRings>> {
    list_items(node)?.iter().map(walk_rings).collect()
}

#[cfg(test)]
mod tests {
    use crate::expanded::geo_property::{
        expanded_geo_node_to_geometry,
        extract_geo_property_geometry,
        extract_location_expiry,
        extract_location_geometry,
        geo_property_instance_geometries,
        validate_expanded_geo_node,
    };
    use andromeda_ngsi_ld_geometry::{dimension::GeometryDimension, geojson::GeometryError, geometry::Geometry, position::PositionError};
    use chrono::{DateTime, Utc};
    use serde_json::json;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    /// Wrap a `location` `GeoProperty` carrying `value` in one instance, without
    /// a `datasetId`.
    fn located(value: &serde_json::Value) -> serde_json::Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{ "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": value }] }]
        }])
    }

    /// Build the expected [`Geometry`] for a known compact-GeoJSON object by
    /// routing through the `from_geojson_value` path; the direct walker must
    /// produce an equal geometry.
    fn geometry_from_compact(compact: &serde_json::Value) -> Geometry {
        Geometry::from_geojson_value(compact).expect("compact geojson decodes")
    }

    /// One `location` instance carrying `expires_at`, when given.
    fn location_instance(expires_at: Option<&str>) -> serde_json::Value {
        let mut instance = json!({ "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [1.0, 2.0]} }] });
        if let Some(raw) = expires_at {
            instance["https://uri.etsi.org/ngsi-ld/expiresAt"] = json!([{"@value": raw}]);
        }
        instance
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("instant parses").with_timezone(&Utc)
    }

    #[test]
    fn an_entity_without_a_location_has_no_location_expiry() {
        assert_eq!(extract_location_expiry(&json!([{"@id": "urn:ngsi-ld:Building:001"}])), None);
    }

    #[test]
    fn a_location_instance_without_an_expiry_keeps_the_location_alive_indefinitely() {
        let entity = json!([{"@id": "urn:x", LOCATION: [location_instance(None)]}]);
        assert_eq!(extract_location_expiry(&entity), None);
        let mixed = json!([{"@id": "urn:x", LOCATION: [location_instance(Some("2030-01-01T00:00:00Z")), location_instance(None)]}]);
        assert_eq!(extract_location_expiry(&mixed), None);
    }

    #[test]
    fn the_latest_expiry_of_the_location_instances_is_recorded() {
        let entity = json!([{"@id": "urn:x", LOCATION: [location_instance(Some("2030-01-01T00:00:00Z")), location_instance(Some("2031-01-01T00:00:00Z"))]}]);
        assert_eq!(extract_location_expiry(&entity), Some(instant("2031-01-01T00:00:00Z")));
    }

    #[test]
    fn an_unparsable_location_expiry_counts_as_no_expiry() {
        let entity = json!([{"@id": "urn:x", LOCATION: [location_instance(Some("garbage")), location_instance(Some("2031-01-01T00:00:00Z"))]}]);
        assert_eq!(extract_location_expiry(&entity), None);
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
            matches!(error, GeometryError::Position(PositionError::NonNumericOrdinate { index: 0 })),
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
            matches!(error, GeometryError::Position(PositionError::NonNumericOrdinate { index: 2 })),
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
            matches!(error, GeometryError::Position(PositionError::NonNumericOrdinate { index: 1 })),
            "got {error:?}"
        );
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

    #[test]
    fn a_geoproperty_written_as_a_literal_object_is_read_as_a_geometry() {
        // Clause 4.7 permits the GeoJSON literal to arrive as an inline object.
        let entity = located(&json!({"type": "Point", "coordinates": [13.4, 52.5]}));
        assert!(extract_location_geometry(&entity).is_some());
    }

    #[test]
    fn a_geoproperty_written_as_a_json_string_is_read_as_a_geometry() {
        // Clause 4.7 permits the GeoJSON document to arrive as a string.
        let entity = located(&json!("{\"type\":\"Point\",\"coordinates\":[13.4,52.5]}"));
        assert!(extract_location_geometry(&entity).is_some());
    }

    #[test]
    fn an_entity_that_carries_no_such_geoproperty_yields_no_geometry() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        assert!(extract_geo_property_geometry(&entity, LOCATION).is_none());
    }

    /// Clause 4.10: a geo restriction is met when *any* instance of the target
    /// `GeoProperty` meets it, so the matching reader yields every instance —
    /// including the tagged ones the default-instance reader deliberately
    /// ignores — in payload order.
    #[test]
    fn every_instance_of_a_multi_dataset_geoproperty_is_yielded_for_matching() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [0.0, 0.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:tagged"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }]
                }
            ]
        }]);
        let instances: Vec<Geometry> = geo_property_instance_geometries(&entity, LOCATION).collect();
        pretty_assertions::assert_eq!(instances.len(), 2);
        pretty_assertions::assert_eq!(instances[0], geometry_from_compact(&json!({"type": "Point", "coordinates": [0.0, 0.0]})));
        pretty_assertions::assert_eq!(instances[1], geometry_from_compact(&json!({"type": "Point", "coordinates": [13.4, 52.5]})));

        // The index-side reader still reduces the same payload to the one
        // instance clause 4.5.16.1 calls the default.
        pretty_assertions::assert_eq!(
            extract_location_geometry(&entity).expect("default instance geometry"),
            geometry_from_compact(&json!({"type": "Point", "coordinates": [13.4, 52.5]}))
        );
    }

    /// An instance whose value is not a geometry is skipped rather than
    /// stopping the walk, exactly as the SQL rendering skips an
    /// `current_attribute_values` row with a null `value_geometry`.
    #[test]
    fn an_instance_carrying_no_geometry_does_not_hide_the_instances_after_it() {
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                { "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": "not geojson" }] },
                { "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }] }
            ]
        }]);
        pretty_assertions::assert_eq!(geo_property_instance_geometries(&entity, LOCATION).count(), 1);
    }

    #[test]
    fn an_entity_carrying_no_such_geoproperty_yields_no_instances_for_matching() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        pretty_assertions::assert_eq!(geo_property_instance_geometries(&entity, LOCATION).count(), 0);
    }

    #[test]
    fn a_geoproperty_whose_value_is_not_geojson_yields_no_geometry() {
        let entity = located(&json!("not geojson"));
        assert!(extract_location_geometry(&entity).is_none());
    }

    #[test]
    fn the_default_instance_is_read_regardless_of_array_order() {
        // Clause 4.5.16.1: the default instance is the one without a datasetId.
        // The tagged instance is placed first to prove selection is by datasetId
        // absence, not by array position: the write/index and read/match paths
        // must pick the same instance so an index hit and a store re-check agree.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [0.0, 0.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:tagged"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }]
                }
            ]
        }]);
        let compact = json!({"type": "Point", "coordinates": [13.4, 52.5]});
        pretty_assertions::assert_eq!(
            extract_location_geometry(&entity).expect("default instance geometry"),
            geometry_from_compact(&compact)
        );
    }

    #[test]
    fn when_every_instance_is_tagged_the_first_is_read() {
        // No datasetId-less instance exists, so no default is defined; the first
        // is taken deterministically rather than the last.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [13.4, 52.5]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:A"}]
                },
                {
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": [0.0, 0.0]} }],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:B"}]
                }
            ]
        }]);
        let compact = json!({"type": "Point", "coordinates": [13.4, 52.5]});
        pretty_assertions::assert_eq!(
            extract_location_geometry(&entity).expect("first tagged geometry"),
            geometry_from_compact(&compact)
        );
    }

    #[test]
    fn the_expanded_node_form_is_read_for_an_arbitrary_geoproperty_iri() {
        // The single reader answers for any GeoProperty IRI, not only the three
        // well-known ones, and handles the fully expanded node form.
        let entity = json!([{
            "@id": "urn:ngsi-ld:Entity:001",
            "https://example.com/site": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [
                        {"@list": [{"@value": 16.37_f64}, {"@value": 48.20_f64}]}
                    ]
                }]
            }]
        }]);
        let compact = json!({"type": "Point", "coordinates": [16.37_f64, 48.20_f64]});
        pretty_assertions::assert_eq!(
            extract_geo_property_geometry(&entity, "https://example.com/site").expect("arbitrary geoproperty"),
            geometry_from_compact(&compact)
        );
    }
}
