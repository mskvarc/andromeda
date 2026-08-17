//! Geometry in the NGSI-LD data model.
//!
//! NGSI-LD (ETSI GS CIM 009 v1.9.1, clause 4.7 and Table 5.2.7-1) defines
//! GeoProperty values as GeoJSON geometries per IETF RFC 7946, in WGS84 /
//! CRS84. That is the representation this type serialises to and from. The
//! PostGIS binary encoding is not a specification artefact and lives outside
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
use geojson::{Geometry as GeoJsonGeometry, GeometryValue};
use jsonld_expandable_core::{JsonValue, ToJsonValue};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

/// A geometry attached to an NGSI-LD GeoProperty (clause 5.2.7).
///
/// In WGS84 / CRS84, as RFC 7946 mandates for GeoJSON without an explicit CRS
/// member, and up to three-dimensional: the optional third ordinate of RFC 7946
/// section 3.1.1 is carried through unchanged. Equality is structural over the
/// form, altitude included, so a change to a height alone is a change to the
/// geometry. There is deliberately no `Eq`/`Hash`, because the coordinates are
/// `f64` and no total order or hash over them would be meaningful.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "GeoJsonGeometry")]
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
    pub fn new(form: GeometryForm) -> Geometry {
        let planar = project(&form);
        Geometry { form, planar }
    }

    /// Build from a parsed GeoJSON geometry.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::Position`] if any position of the geometry
    /// carries fewer than two or more than three ordinates, and
    /// [`GeometryError::GeometryCollectionUnsupported`] for a GeometryCollection,
    /// which ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes.
    pub fn from_geojson(geometry: &GeoJsonGeometry) -> Result<Geometry, GeometryError> {
        Ok(Geometry::new(GeometryForm::from_geojson(&geometry.value)?))
    }

    /// Build from an arbitrary JSON value that is expected to encode a GeoJSON
    /// geometry object.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::GeojsonValue`] if `value` is not a GeoJSON
    /// geometry object, [`GeometryError::Position`] if one of its positions has
    /// an ordinate count RFC 7946 gives no meaning to, or
    /// [`GeometryError::GeometryCollectionUnsupported`] if it is a
    /// GeometryCollection.
    pub fn from_geojson_value(value: &Value) -> Result<Geometry, GeometryError> {
        // clone: `serde_json::from_value` consumes the value it decodes
        let geometry: GeoJsonGeometry = serde_json::from_value(value.clone())?;
        Geometry::from_geojson(&geometry)
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
    /// Returns [`GeometryError::CoordinateOutOfRange`] when an ordinate cannot
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
    /// ordinate count it arrived with, every polygon ring exactly as written.
    pub const fn form(&self) -> &GeometryForm {
        &self.form
    }

    /// The horizontal projection the clause 4.10 predicates are evaluated over.
    pub const fn planar(&self) -> &geo::Geometry<f64> {
        &self.planar
    }

    /// Whether every position of this geometry carries an altitude.
    pub fn dimension(&self) -> GeometryDimension {
        self.form.dimension()
    }

    /// Render as a GeoJSON geometry for the wire.
    ///
    /// Infallible, and byte-faithful to what was received: a position that
    /// arrived with two ordinates renders with two, one that arrived with three
    /// renders with three, and an open polygon ring stays open.
    pub fn to_geojson(&self) -> GeoJsonGeometry {
        GeoJsonGeometry::new(self.form.to_geojson())
    }

    /// Axis-aligned bounding rectangle of the horizontal projection, or `None`
    /// for a geometry with no coordinates, such as a Polygon with no rings.
    pub fn bounding_rect(&self) -> Option<geo::Rect<f64>> {
        self.planar.bounding_rect()
    }
}

/// Narrow arbitrary-precision JSON coordinate numbers to the finite binary64
/// domain used by `geo`, `geojson` and PostGIS geometry operations.
fn coordinates_as_f64(value: Value) -> Result<Value, GeometryError> {
    match value {
        Value::Number(number) => {
            let coordinate = number
                .as_f64()
                .filter(|coordinate| coordinate.is_finite())
                .ok_or(GeometryError::CoordinateOutOfRange)?;
            let number = serde_json::Number::from_f64(coordinate).ok_or(GeometryError::CoordinateOutOfRange)?;
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

impl TryFrom<GeoJsonGeometry> for Geometry {
    type Error = GeometryError;

    fn try_from(geometry: GeoJsonGeometry) -> Result<Geometry, GeometryError> {
        Geometry::from_geojson(&geometry)
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
        // `From<&GeometryValue> for serde_json::Value` is infallible, so unlike
        // a `serde_json::to_value` round trip there is no `Result` to discard
        // inside this signature, which has no way to report a failure. The
        // second hop is `jsonld-expandable-core`'s own `serde_json` backend.
        let value: &GeometryValue = &self.form.to_geojson();
        ToJsonValue::<V>::to_json_value(&Value::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::PositionError;

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

    fn open_elevated_polygon_json() -> Value {
        serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0, 10.0], [5.0, 2.0, 10.0], [5.0, 8.0, 10.0], [1.0, 8.0, 10.0]]]
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
    fn a_coordinate_outside_the_finite_f64_domain_is_rejected_at_the_geometry_boundary() {
        let coordinates: Value = serde_json::from_str("[1e400,52.5]").expect("JSON number parses");
        let error = Geometry::from_type_and_coordinates(GeometryType::Point, coordinates).expect_err("coordinate must be rejected");
        assert!(matches!(error, GeometryError::CoordinateOutOfRange), "got {error:?}");
    }

    #[test]
    fn an_elevated_point_serialises_back_to_the_json_it_arrived_as() {
        let geometry = Geometry::from_geojson_value(&elevated_point_json()).expect("elevated point parses");
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::ThreeD);
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), elevated_point_json());
    }

    #[test]
    fn an_open_elevated_polygon_ring_round_trips_without_a_fabricated_closing_position() {
        let geometry = Geometry::from_geojson_value(&open_elevated_polygon_json()).expect("open polygon parses");
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), open_elevated_polygon_json());

        // The projection closes its own copy, as `geo::Polygon` requires, which
        // is what leaves the wire form free to stay open.
        let geo::Geometry::Polygon(projected) = geometry.planar() else {
            unreachable!("expected a Polygon projection");
        };
        pretty_assertions::assert_eq!(projected.exterior().0.len(), 5);
        pretty_assertions::assert_eq!(projected.exterior().0.first(), projected.exterior().0.last());
    }

    #[test]
    fn a_malformed_geometry_value_is_a_typed_error() {
        let error = Geometry::from_geojson_value(&serde_json::json!({ "type": "Nonsense", "coordinates": [] })).expect_err("must reject");
        assert!(matches!(error, GeometryError::GeojsonValue(_)), "got {error:?}");
    }

    #[test]
    fn a_point_with_four_ordinates_is_a_position_error() {
        let error = Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0] })).expect_err("must reject");
        assert!(
            matches!(error, GeometryError::Position(PositionError::TooManyOrdinates { ordinates: 4 })),
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
}
