//! Geometry in the NGSI-LD data model.
//!
//! NGSI-LD (ETSI GS CIM 009 v1.9.1, clause 4.7 and Table 5.2.7-1) defines
//! `GeoProperty` values as `GeoJSON` geometries per IETF RFC 7946, in WGS84 /
//! CRS84. That is the representation this type serialises to and from. The
//! `PostGIS` binary encoding is not a specification artefact and lives outside
//! this crate, in `andromeda-postgresql-encoding`, reached only by the store
//! and by the statement renderers that bind a reference geometry.
//!
//! A geometry is two values of one thing: the wire form it arrived as, kept
//! verbatim down to the altitude and the arity of every position, and its
//! projection onto the horizontal plane, which is what clause 4.10 predicates
//! are evaluated over. The projection is computed once, at construction, so
//! neither an entity write nor a subscription match pays for it repeatedly.

use crate::{dimension::GeometryDimension, form::GeometryForm, geojson::GeometryError, geometry_type::GeometryType, planar::project};
use geo::algorithm::BoundingRect;
use geojson::Geometry as GeoJsonGeometry;
use jsonld_expandable_core::{JsonValue, ToJsonValue};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

/// A geometry attached to an NGSI-LD `GeoProperty` (clause 5.2.7).
///
/// In WGS84 / CRS84, as RFC 7946 mandates for `GeoJSON` without an explicit CRS
/// member, and up to three-dimensional: the optional third ordinate of RFC 7946
/// section 3.1.1 is carried through unchanged. Equality is structural over the
/// form, altitude included, so a change to a height alone is a change to the
/// geometry. There is deliberately no `Eq`/`Hash`, because the coordinates are
/// `f64` and no total order or hash over them would be meaningful.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "Value")]
pub struct Geometry {
    /// The geometry as received, and as it will be rendered back.
    form: GeometryForm,
    /// The horizontal projection, cached for predicate evaluation.
    planar: geo::Geometry<f64>,
}

impl Geometry {
    /// Build from a geometry form, projecting it onto the horizontal plane.
    ///
    /// Infallible: every form has a projection, including the degenerate ones
    /// RFC 7946 permits.
    #[must_use]
    pub fn new(form: GeometryForm) -> Geometry {
        let planar = project(&form);
        Geometry { form, planar }
    }

    /// Build from an arbitrary JSON value that is expected to encode a `GeoJSON`
    /// geometry object.
    ///
    /// # Errors
    ///
    /// Returns the [`GeometryError`] naming what is missing or misplaced if
    /// `value` is not a `GeoJSON` geometry object, [`GeometryError::Position`] if one of its positions has
    /// an ordinate count RFC 7946 gives no meaning to or lies off the CRS84
    /// axes, [`GeometryError::LineString`] or [`GeometryError::LinearRing`]
    /// for a line or ring RFC 7946 does not admit, or
    /// [`GeometryError::GeometryCollectionUnsupported`] if it is a
    /// `GeometryCollection`.
    pub fn from_geojson_value(value: &Value) -> Result<Geometry, GeometryError> {
        Ok(Geometry::new(GeometryForm::from_compact_value(value)?))
    }

    /// Build from a `GeoProperty` value as written: a `GeoJSON` geometry
    /// object, or a string holding one, which ETSI GS CIM 009 v1.9.1 clause
    /// 4.7.2 accepts "if and only if it can be parsed into a JSON Object" that
    /// is a valid RFC 7946 geometry.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::EncodedNotJson`] for a string that is no JSON
    /// text, and otherwise the errors of [`Geometry::from_geojson_value`].
    pub fn from_written_value(value: &Value) -> Result<Geometry, GeometryError> {
        match value {
            Value::String(text) => Geometry::from_geojson_value(&serde_json::from_str::<Value>(text).map_err(GeometryError::EncodedNotJson)?),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => Geometry::from_geojson_value(value),
        }
    }

    /// Build from the separate geometry type and coordinates used by NGSI-LD
    /// query parameters and subscription `geoQ` objects.
    ///
    /// Coordinates are explicitly narrowed to finite `f64` values at this
    /// domain boundary, then inserted into a JSON object without passing through
    /// a generic serializer. This matters when `serde_json` arbitrary precision
    /// is enabled: a parsed number carries a private raw-number token that an
    /// incompatible serializer can otherwise expose as an object instead of a
    /// coordinate number.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFiniteCoordinate`] when an ordinate cannot
    /// be represented as a finite `f64`, or the same typed errors as
    /// [`Geometry::from_geojson_value`] when the coordinates do not form the
    /// requested RFC 7946 geometry.
    pub fn from_type_and_coordinates(geometry_type: GeometryType, coordinates: Value) -> Result<Geometry, GeometryError> {
        let mut object = Map::new();
        object.insert("type".to_owned(), Value::String(geometry_type.to_string()));
        object.insert("coordinates".to_owned(), coordinates_as_f64(coordinates)?);
        Geometry::from_geojson_value(&Value::Object(object))
    }

    /// The geometry as received: every position with the altitude and the
    /// ordinate count it arrived with.
    #[must_use]
    pub const fn form(&self) -> &GeometryForm {
        &self.form
    }

    /// The horizontal projection the clause 4.10 predicates are evaluated over.
    #[must_use]
    pub const fn planar(&self) -> &geo::Geometry<f64> {
        &self.planar
    }

    /// Whether every position of this geometry carries an altitude.
    #[must_use]
    pub fn dimension(&self) -> GeometryDimension {
        self.form.dimension()
    }

    /// Render as a `GeoJSON` geometry for the wire.
    ///
    /// Infallible, and byte-faithful to what was received: a position that
    /// arrived with two ordinates renders with two, and one that arrived with
    /// three renders with three.
    #[must_use]
    pub fn to_geojson(&self) -> GeoJsonGeometry {
        GeoJsonGeometry::new(self.form.to_geojson())
    }

    /// Render as the compact `GeoJSON` geometry object, the content of the JSON
    /// literal an expanded `GeoProperty` value holds.
    #[must_use]
    pub fn to_geojson_value(&self) -> Value {
        Value::from(&self.form.to_geojson())
    }

    /// Axis-aligned bounding rectangle of the horizontal projection, or `None`
    /// for a geometry with no coordinates, such as a Polygon with no rings.
    #[must_use]
    pub fn bounding_rect(&self) -> Option<geo::Rect<f64>> {
        self.planar.bounding_rect()
    }
}

/// Narrow arbitrary-precision JSON coordinate numbers to the finite binary64
/// domain used by `geo`, `geojson` and `PostGIS` geometry operations.
fn coordinates_as_f64(value: Value) -> Result<Value, GeometryError> {
    match value {
        Value::Number(number) => {
            let coordinate = number
                .as_f64()
                .filter(|coordinate| coordinate.is_finite())
                .ok_or(GeometryError::NonFiniteCoordinate)?;
            let number = serde_json::Number::from_f64(coordinate).ok_or(GeometryError::NonFiniteCoordinate)?;
            Ok(Value::Number(number))
        }
        Value::Array(values) => values.into_iter().map(coordinates_as_f64).collect::<Result<Vec<_>, _>>().map(Value::Array),
        Value::Null | Value::Bool(_) | Value::String(_) | Value::Object(_) => Ok(value),
    }
}

// Hand-written because the field to compare is chosen, not all of them:
// `planar` is a pure projection of `form`, so comparing it as well would be
// redundant work on the subscription matching path. The sanctioned derive for a
// skipped field is `educe`, and a proc-macro dependency in this leaf crate is
// not worth one skipped field.
impl PartialEq for Geometry {
    fn eq(&self, other: &Geometry) -> bool {
        self.form == other.form
    }
}

// A decoded geometry is read off the JSON tree serde buffered for it rather
// than through the `geojson` crate's own decoding, which would buffer the whole
// geometry once more for its foreign members before reading a position.
impl TryFrom<Value> for Geometry {
    type Error = GeometryError;

    fn try_from(value: Value) -> Result<Geometry, GeometryError> {
        Geometry::from_geojson_value(&value)
    }
}

// Hand-written rather than `#[serde(into = "GeoJsonGeometry")]`: that container
// attribute requires `Clone` and clones the whole geometry on every serialise,
// which is the hot path for every entity response carrying a GeoProperty.
impl Serialize for Geometry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_geojson().serialize(serializer)
    }
}

impl<V: JsonValue> ToJsonValue<V> for Geometry {
    fn to_json_value(&self) -> V {
        // The GeoJSON object is built infallibly, so unlike a
        // `serde_json::to_value` round trip there is no `Result` to discard
        // inside this signature, which has no way to report a failure. The
        // second hop is `jsonld-expandable-core`'s own `serde_json` backend.
        ToJsonValue::<V>::to_json_value(&self.to_geojson_value())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        coordinate_path::CoordinatePath,
        dimension::GeometryDimension,
        geojson::GeometryError,
        geometry::Geometry,
        geometry_type::GeometryType,
        linear_ring::LinearRingError,
        position::PositionError,
    };
    use jsonld_expandable_core::ToJsonValue;
    use serde_json::Value;

    /// Where a refused position sits and what is wrong with it.
    fn refused_position(error: &GeometryError) -> (CoordinatePath, PositionError) {
        match error {
            GeometryError::Position { at, source } => (at.clone(), *source), // clone: the assertion owns the path it compares
            other @ (GeometryError::LineString { .. }
            | GeometryError::LinearRing { .. }
            | GeometryError::Misnested { .. }
            | GeometryError::NotAnObject
            | GeometryError::EncodedNotJson(_)
            | GeometryError::MissingType
            | GeometryError::UnknownType(_)
            | GeometryError::MissingCoordinates
            | GeometryError::MissingGeometries
            | GeometryError::InvalidBbox
            | GeometryError::NotAnArray { .. }
            | GeometryError::NonNumericOrdinate { .. }
            | GeometryError::NonFiniteCoordinate
            | GeometryError::GeometryCollectionUnsupported) => {
                panic!("expected a refused position, got {other:?}")
            }
        }
    }

    fn point_json() -> Value {
        serde_json::json!({ "type": "Point", "coordinates": [13.38, 52.52] })
    }

    fn elevated_point_json() -> Value {
        serde_json::json!({ "type": "Point", "coordinates": [13.38, 52.52, 34.5] })
    }

    fn polygon_json() -> Value {
        serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0], [5.0, 2.0], [5.0, 8.0], [1.0, 8.0], [1.0, 2.0]]]
        })
    }

    fn elevated_polygon_json() -> Value {
        serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0, 10.0], [5.0, 2.0, 10.0], [5.0, 8.0, 10.0], [1.0, 8.0, 10.0], [1.0, 2.0, 10.0]]]
        })
    }

    fn collection_json() -> Value {
        serde_json::json!({
            "type": "GeometryCollection",
            "geometries": [
                { "type": "Point", "coordinates": [100.0, 0.0, 7.0] },
                { "type": "LineString", "coordinates": [[101.0, 0.0], [102.0, 1.0]] }
            ]
        })
    }

    #[test]
    fn a_geometry_collection_is_refused_as_a_geoproperty_geometry() {
        // ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes GeometryCollection from
        // the geometries a GeoProperty value may take.
        let error = Geometry::from_geojson_value(&collection_json()).expect_err("a collection is not an allowed geometry");
        assert!(matches!(error, GeometryError::GeometryCollectionUnsupported), "got {error:?}");
    }

    #[test]
    fn deserialising_a_geometry_collection_fails_through_the_serde_path() {
        // The rejection reaches serde as well, so no request body carrying a
        // collection decodes into a `Geometry` anywhere in the broker.
        let error = serde_json::from_value::<Geometry>(collection_json()).expect_err("serde refuses a collection");
        assert!(
            error.to_string().contains("GeometryCollection is not an allowed GeoProperty geometry"),
            "got {error}"
        );
    }

    #[test]
    fn a_geojson_point_round_trips_through_the_domain_type() {
        let geometry = Geometry::from_geojson_value(&point_json()).expect("point parses");
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), point_json());
    }

    #[test]
    fn separate_type_and_arbitrary_precision_coordinates_decode_without_exposing_the_raw_number_token() {
        let coordinates: Value = serde_json::from_str("[0.100000000000000005,52.5]").expect("coordinates parse");
        let geometry = Geometry::from_type_and_coordinates(GeometryType::Point, coordinates).expect("point parses");
        pretty_assertions::assert_eq!(geometry.to_geojson().value.type_name(), "Point");
    }

    #[test]
    fn a_polygon_vertex_past_a_pole_is_a_position_error() {
        let polygon = serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0], [5.0, 2.0], [5.0, 91.0], [1.0, 2.0]]]
        });
        let error = Geometry::from_geojson_value(&polygon).expect_err("a vertex past the pole is no position");
        pretty_assertions::assert_eq!(
            refused_position(&error),
            (CoordinatePath::new(vec![0, 2]), PositionError::LatitudeOutOfRange { latitude: 91.0 })
        );
        // The rendering is the BadRequestData detail a client reads: what is
        // wrong, by which rule, and which vertex to fix.
        pretty_assertions::assert_eq!(
            error.to_string(),
            "GeoJSON position latitude 91 is outside [-90, 90]; RFC 7946 section 4 places positions in WGS 84 longitude and latitude, in decimal degrees (at coordinates[0][2])"
        );
    }

    #[test]
    fn a_hole_vertex_of_a_later_polygon_past_the_antimeridian_is_a_position_error() {
        let multi_polygon = serde_json::json!({
            "type": "MultiPolygon",
            "coordinates": [
                [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]],
                [
                    [[10.0, 10.0], [20.0, 10.0], [20.0, 20.0], [10.0, 10.0]],
                    [[12.0, 12.0], [-181.0, 12.0], [14.0, 14.0], [12.0, 12.0]]
                ]
            ]
        });
        let error = Geometry::from_geojson_value(&multi_polygon).expect_err("a hole vertex past the antimeridian is no position");
        pretty_assertions::assert_eq!(
            refused_position(&error),
            (CoordinatePath::new(vec![1, 1, 1]), PositionError::LongitudeOutOfRange { longitude: -181.0 })
        );
    }

    #[test]
    fn every_geometry_type_names_the_one_position_off_the_crs84_axes() {
        for (geometry, at, source) in [
            (
                serde_json::json!({ "type": "LineString", "coordinates": [[0.0, 0.0], [0.0, 90.5]] }),
                vec![1],
                PositionError::LatitudeOutOfRange { latitude: 90.5 },
            ),
            (
                serde_json::json!({ "type": "MultiPoint", "coordinates": [[0.0, 0.0], [190.0, 0.0]] }),
                vec![1],
                PositionError::LongitudeOutOfRange { longitude: 190.0 },
            ),
            (
                serde_json::json!({ "type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], [[0.0, 0.0], [0.0, -95.0]]] }),
                vec![1, 1],
                PositionError::LatitudeOutOfRange { latitude: -95.0 },
            ),
        ] {
            let error = Geometry::from_geojson_value(&geometry).expect_err("one position off the axes refuses the geometry");
            pretty_assertions::assert_eq!(refused_position(&error), (CoordinatePath::new(at), source), "{geometry}");
        }
    }

    #[test]
    fn a_reference_geometry_past_a_pole_is_refused_like_a_payload_geometry() {
        // The coordinates of the informative ETSI GS CIM 009 v1.9.1 Annex C.4
        // example, which name a latitude of 100.
        let error = Geometry::from_type_and_coordinates(GeometryType::Point, serde_json::json!([-1, 100])).expect_err("latitude 100 is no position");
        pretty_assertions::assert_eq!(
            refused_position(&error),
            (CoordinatePath::default(), PositionError::LatitudeOutOfRange { latitude: 100.0 })
        );
    }

    #[test]
    fn a_coordinate_outside_the_finite_f64_domain_is_rejected_at_the_geometry_boundary() {
        let coordinates: Value = serde_json::from_str("[1e400,52.5]").expect("JSON number parses");
        let error = Geometry::from_type_and_coordinates(GeometryType::Point, coordinates).expect_err("coordinate must be rejected");
        assert!(matches!(error, GeometryError::NonFiniteCoordinate), "got {error:?}");
    }

    #[test]
    fn an_elevated_point_serialises_back_to_the_json_it_arrived_as() {
        let geometry = Geometry::from_geojson_value(&elevated_point_json()).expect("elevated point parses");
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::ThreeD);
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), elevated_point_json());
    }

    #[test]
    fn an_elevated_polygon_round_trips_with_every_altitude() {
        let geometry = Geometry::from_geojson_value(&elevated_polygon_json()).expect("closed polygon parses");
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::ThreeD);
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), elevated_polygon_json());
    }

    #[test]
    fn an_open_polygon_ring_is_refused_rather_than_closed() {
        // RFC 7946 section 3.1.6: the ring's ends MUST be identical, and which
        // closing position a client meant is not the broker's to guess.
        let open = serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0], [5.0, 2.0], [5.0, 8.0], [1.0, 8.0]]]
        });
        let error = Geometry::from_geojson_value(&open).expect_err("an open ring is no ring");
        assert!(
            matches!(error, GeometryError::LinearRing { ref at, source: LinearRingError::Open } if *at == CoordinatePath::new(vec![0])),
            "got {error:?}"
        );
    }

    #[test]
    fn a_malformed_geometry_value_is_a_typed_error() {
        let error = Geometry::from_geojson_value(&serde_json::json!({ "type": "Nonsense", "coordinates": [] })).expect_err("must reject");
        assert!(matches!(error, GeometryError::UnknownType(_)), "got {error:?}");
    }

    #[test]
    fn a_point_with_four_ordinates_is_a_position_error() {
        let error = Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0] })).expect_err("must reject");
        assert!(
            matches!(
                error,
                GeometryError::Position {
                    source: PositionError::TooManyOrdinates { ordinates: 4 },
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn the_bounding_rectangle_of_a_polygon_covers_every_vertex() {
        let geometry = Geometry::from_geojson_value(&polygon_json()).expect("polygon parses");
        let rect = geometry.bounding_rect().expect("non-empty geometry has a bounding rect");
        pretty_assertions::assert_eq!(rect.min(), geo::Coord { x: 1.0, y: 2.0 });
        pretty_assertions::assert_eq!(rect.max(), geo::Coord { x: 5.0, y: 8.0 });
    }

    #[test]
    fn geometry_serde_round_trips_without_loss() {
        let geometry = Geometry::from_geojson_value(&polygon_json()).expect("polygon parses");
        let encoded = serde_json::to_value(&geometry).expect("serialises");
        let decoded: Geometry = serde_json::from_value(encoded).expect("deserialises");
        pretty_assertions::assert_eq!(decoded, geometry);
    }

    #[test]
    fn two_geometries_differing_only_in_altitude_are_not_equal() {
        let flat = Geometry::from_geojson_value(&point_json()).expect("point parses");
        let elevated = Geometry::from_geojson_value(&elevated_point_json()).expect("elevated point parses");
        assert!(flat != elevated, "an altitude-only difference is a difference");
    }

    #[test]
    fn a_geometry_renders_as_the_json_value_the_expanded_model_carries() {
        let geometry = Geometry::from_geojson_value(&elevated_point_json()).expect("elevated point parses");
        let rendered: Value = ToJsonValue::<Value>::to_json_value(&geometry);
        pretty_assertions::assert_eq!(rendered, elevated_point_json());
    }

    /// Clause 4.7.2: a string is a geometry if and only if it parses into a
    /// valid geometry object.
    #[test]
    fn a_written_string_is_a_geometry_only_when_it_parses_into_one() {
        let encoded = Value::String(point_json().to_string());
        pretty_assertions::assert_eq!(
            Geometry::from_written_value(&encoded).expect("an encoded point"),
            Geometry::from_geojson_value(&point_json()).expect("a point")
        );
        let not_json = Geometry::from_written_value(&Value::String("POINT(1 2)".to_owned())).expect_err("no JSON text");
        assert!(matches!(not_json, GeometryError::EncodedNotJson(_)), "{not_json:?}");
        let not_an_object = Geometry::from_written_value(&Value::String("[1, 2]".to_owned())).expect_err("no object");
        assert!(matches!(not_an_object, GeometryError::NotAnObject), "{not_an_object:?}");
        let not_a_geometry = Geometry::from_written_value(&Value::String("{\"a\": 1}".to_owned())).expect_err("no geometry");
        assert!(matches!(not_a_geometry, GeometryError::MissingType), "{not_a_geometry:?}");
    }
}
