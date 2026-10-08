//! Geometry in the NGSI-LD data model.
//!
//! NGSI-LD (ETSI GS CIM 009 v1.9.1, clause 4.7 and Table 5.2.7-1) defines
//! `GeoProperty` values as `GeoJSON` geometries per IETF RFC 7946, in WGS84 /
//! CRS84. That is the representation this type serialises to and from. The
//! `PostGIS` binary encoding is not a specification artefact and lives outside
//! this crate, in `andromeda-postgresql-encoding`, reached only by the store
//! and by the statement renderers that bind a reference geometry.
//!
//! A geometry is the wire form it arrived as, kept verbatim down to the
//! altitude and the arity of every position. Its projection onto the
//! horizontal plane, which is what clause 4.10 predicates are evaluated over,
//! is a separate [`PlanarGeometry`] computed when a spatial operation needs it,
//! so a geometry held in an entity carries its coordinates once.

use crate::{
    dimension::GeometryDimension,
    form::GeometryForm,
    geojson::GeometryError,
    geojson_members::GeoJsonMembers,
    planar::PlanarGeometry,
    written_geometry::WrittenGeometry,
};
use geojson::Geometry as GeoJsonGeometry;

/// A geometry attached to an NGSI-LD `GeoProperty` (clause 5.2.7).
///
/// In WGS84 / CRS84, as RFC 7946 mandates for `GeoJSON` without an explicit CRS
/// member, and up to three-dimensional: the optional third ordinate of RFC 7946
/// section 3.1.1 is carried through unchanged. Equality is structural over the
/// form, altitude included, so a change to a height alone is a change to the
/// geometry. There is deliberately no `Eq`/`Hash`, because the coordinates are
/// `f64` and no total order or hash over them would be meaningful.
///
/// Decoding reads a compact `GeoJSON` geometry object straight from the
/// deserializer (see [`crate::written_geometry`]), never through a JSON tree.
/// The `bbox` and the foreign members written beside the positions (RFC 7946
/// sections 5 and 6.1) are kept and rendered back (ETSI GS CIM 009 v1.9.1
/// clause 4.6.4); a geometry without them holds nothing for them.
#[derive(Debug, Clone, PartialEq)]
pub struct Geometry {
    /// The geometry as received, and as it will be rendered back.
    form: GeometryForm,
    /// The `bbox` and the foreign members, when either was written.
    members: Option<Box<GeoJsonMembers>>,
}

impl Geometry {
    /// Build from a geometry form, with no member beside its positions.
    #[must_use]
    pub const fn new(form: GeometryForm) -> Geometry {
        Geometry { form, members: None }
    }

    /// Build from a geometry form and the members written beside it.
    #[must_use]
    pub fn with_members(form: GeometryForm, members: Option<GeoJsonMembers>) -> Geometry {
        Geometry {
            form,
            members: members.map(Box::new),
        }
    }

    /// Build from the text of a compact `GeoJSON` geometry object, which ETSI
    /// GS CIM 009 v1.9.1 clause 4.7.2 also accepts as the content of a string
    /// "if and only if it can be parsed into a JSON Object" that is a valid
    /// RFC 7946 geometry.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::EncodedNotJson`] for text that is no single JSON
    /// value, the [`GeometryError`] naming what is missing or misplaced if the
    /// value is not a `GeoJSON` geometry object, [`GeometryError::Position`] if
    /// one of its positions has an ordinate count RFC 7946 gives no meaning to
    /// or lies off the CRS84 axes, [`GeometryError::LineString`] or
    /// [`GeometryError::LinearRing`] for a line or ring RFC 7946 does not
    /// admit, or [`GeometryError::GeometryCollectionUnsupported`] if it is a
    /// `GeometryCollection`.
    pub fn from_geojson_text(text: &str) -> Result<Geometry, GeometryError> {
        serde_json::from_str::<WrittenGeometry>(text)
            .map_err(GeometryError::EncodedNotJson)?
            .into_geometry()
    }

    /// The geometry as received: every position with the altitude and the
    /// ordinate count it arrived with.
    #[must_use]
    pub const fn form(&self) -> &GeometryForm {
        &self.form
    }

    /// The `bbox` and the foreign members written beside the positions, when
    /// either was.
    #[must_use]
    pub fn members(&self) -> Option<&GeoJsonMembers> {
        self.members.as_deref()
    }

    /// The form and the members, apart.
    #[must_use]
    pub fn into_parts(self) -> (GeometryForm, Option<GeoJsonMembers>) {
        (self.form, self.members.map(|members| *members))
    }

    /// Project onto the horizontal plane the clause 4.10 predicates are
    /// evaluated over.
    ///
    /// Computed on every call: a caller evaluating this geometry against
    /// several others projects it once and keeps the result.
    #[must_use]
    pub fn planar(&self) -> PlanarGeometry {
        PlanarGeometry::project(&self.form)
    }

    /// Whether every position of this geometry carries an altitude.
    #[must_use]
    pub fn dimension(&self) -> GeometryDimension {
        self.form.dimension()
    }

    /// The geometry as the `geojson` crate holds one, with its `bbox`.
    ///
    /// Infallible, and faithful to the positions: a position that arrived with
    /// two ordinates renders with two, and one that arrived with three renders
    /// with three. The foreign members describe no position and are left out;
    /// the geometry's own [`Serialize`](serde::Serialize) writes them.
    #[must_use]
    pub fn to_geojson(&self) -> GeoJsonGeometry {
        let mut geometry = GeoJsonGeometry::new(self.form.to_geojson());
        geometry.bbox = self.members().and_then(GeoJsonMembers::bbox).map(|bbox| bbox.ordinates().collect());
        geometry
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        coordinate_path::CoordinatePath,
        dimension::GeometryDimension,
        form::GeometryForm,
        geojson::GeometryError,
        geometry::Geometry,
        linear_ring::LinearRingError,
        position::PositionError,
    };
    use serde_json::Value;

    /// Read `value` as the compact `GeoJSON` text it is written as.
    fn read(value: &Value) -> Result<Geometry, GeometryError> {
        Geometry::from_geojson_text(&value.to_string())
    }

    /// Where a refused position sits and what is wrong with it.
    fn refused_position(error: &GeometryError) -> (CoordinatePath, PositionError) {
        match error {
            GeometryError::Position { at, source } => (at.clone(), *source), // clone: the assertion owns the path it compares
            other @ (GeometryError::LineString { .. }
            | GeometryError::LinearRing { .. }
            | GeometryError::MixedArity { .. }
            | GeometryError::Misnested { .. }
            | GeometryError::NotAnObject
            | GeometryError::EncodedNotJson(_)
            | GeometryError::MissingType
            | GeometryError::UnknownType(_)
            | GeometryError::MissingCoordinates
            | GeometryError::MissingGeometries
            | GeometryError::InvalidBbox(_)
            | GeometryError::NotAnArray { .. }
            | GeometryError::NonNumericOrdinate { .. }
            | GeometryError::GeometryCollectionUnsupported
            | GeometryError::ForeignMember(_)) => {
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
        let error = read(&collection_json()).expect_err("a collection is not an allowed geometry");
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
        let geometry = read(&point_json()).expect("point parses");
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), point_json());
    }

    #[test]
    fn a_polygon_vertex_past_a_pole_is_a_position_error() {
        let polygon = serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0], [5.0, 2.0], [5.0, 91.0], [1.0, 2.0]]]
        });
        let error = read(&polygon).expect_err("a vertex past the pole is no position");
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
        let error = read(&multi_polygon).expect_err("a hole vertex past the antimeridian is no position");
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
            let error = read(&geometry).expect_err("one position off the axes refuses the geometry");
            pretty_assertions::assert_eq!(refused_position(&error), (CoordinatePath::new(at), source), "{geometry}");
        }
    }

    #[test]
    fn a_coordinate_outside_the_finite_f64_domain_is_no_ordinate() {
        let error = Geometry::from_geojson_text(r#"{"type": "Point", "coordinates": [1e400, 52.5]}"#).expect_err("coordinate must be rejected");
        assert!(matches!(error, GeometryError::NonNumericOrdinate { .. }), "got {error:?}");
    }

    #[test]
    fn an_elevated_point_serialises_back_to_the_json_it_arrived_as() {
        let geometry = read(&elevated_point_json()).expect("elevated point parses");
        pretty_assertions::assert_eq!(geometry.dimension(), GeometryDimension::ThreeD);
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), elevated_point_json());
    }

    #[test]
    fn an_elevated_polygon_round_trips_with_every_altitude() {
        let geometry = read(&elevated_polygon_json()).expect("closed polygon parses");
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
        let error = read(&open).expect_err("an open ring is no ring");
        assert!(
            matches!(error, GeometryError::LinearRing { ref at, source: LinearRingError::Open } if *at == CoordinatePath::new(vec![0])),
            "got {error:?}"
        );
    }

    #[test]
    fn a_malformed_geometry_value_is_a_typed_error() {
        let error = read(&serde_json::json!({ "type": "Nonsense", "coordinates": [] })).expect_err("must reject");
        assert!(matches!(error, GeometryError::UnknownType(_)), "got {error:?}");
    }

    #[test]
    fn a_line_string_mixing_two_and_three_ordinate_positions_is_refused() {
        let error = Geometry::from_geojson_text(r#"{"type": "LineString", "coordinates": [[13.4, 52.52], [13.5, 52.53, 250.0]]}"#)
            .expect_err("positions disagreeing on altitude make no geometry");
        assert!(
            matches!(error, GeometryError::MixedArity { ref at } if *at == CoordinatePath::new(vec![1])),
            "got {error:?}"
        );
    }

    #[test]
    fn a_point_with_four_ordinates_is_a_position_error() {
        let error = read(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0] })).expect_err("must reject");
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
        let geometry = read(&polygon_json()).expect("polygon parses");
        let rect = geometry.planar().bounding_rect().expect("non-empty geometry has a bounding rect");
        pretty_assertions::assert_eq!(rect.min(), geo::Coord { x: 1.0, y: 2.0 });
        pretty_assertions::assert_eq!(rect.max(), geo::Coord { x: 5.0, y: 8.0 });
    }

    #[test]
    fn geometry_serde_round_trips_without_loss() {
        let geometry = read(&polygon_json()).expect("polygon parses");
        let encoded = serde_json::to_value(&geometry).expect("serialises");
        let decoded: Geometry = serde_json::from_value(encoded).expect("deserialises");
        pretty_assertions::assert_eq!(decoded, geometry);
    }

    #[test]
    fn two_geometries_differing_only_in_altitude_are_not_equal() {
        let flat = read(&point_json()).expect("point parses");
        let elevated = read(&elevated_point_json()).expect("elevated point parses");
        assert!(flat != elevated, "an altitude-only difference is a difference");
    }

    /// IETF RFC 7946 sections 5 and 6.1, ETSI GS CIM 009 v1.9.1 clause 4.6.4:
    /// the box and the foreign members render back beside the coordinates.
    #[test]
    fn a_geometry_with_members_serialises_them_beside_its_coordinates() {
        let written = serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0], "bbox": [1.0, 2.0, 1.0, 2.0], "title": "x"});
        let geometry = read(&written).expect("a point");
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), written);
    }

    /// A geometry without members holds one empty pointer for them.
    #[test]
    fn a_geometry_without_members_allocates_no_member_box() {
        let geometry = read(&point_json()).expect("a point");
        assert!(geometry.members().is_none());
        assert!(size_of::<Geometry>() <= size_of::<GeometryForm>() + size_of::<usize>());
    }

    /// Clause 4.7.2: a string is a geometry if and only if it parses into a
    /// valid geometry object.
    #[test]
    fn a_text_is_a_geometry_only_when_it_parses_into_one() {
        let from_text = Geometry::from_geojson_text(&point_json().to_string()).expect("a point");
        pretty_assertions::assert_eq!(from_text, serde_json::from_value::<Geometry>(point_json()).expect("a point"));
        let not_json = Geometry::from_geojson_text("POINT(1 2)").expect_err("no JSON text");
        assert!(matches!(not_json, GeometryError::EncodedNotJson(_)), "{not_json:?}");
        let trailing = Geometry::from_geojson_text(r#"{"type": "Point", "coordinates": [1, 2]} x"#).expect_err("trailing characters");
        assert!(matches!(trailing, GeometryError::EncodedNotJson(_)), "{trailing:?}");
        let not_an_object = Geometry::from_geojson_text("[1, 2]").expect_err("no object");
        assert!(matches!(not_an_object, GeometryError::NotAnObject), "{not_an_object:?}");
        let not_a_geometry = Geometry::from_geojson_text("{\"a\": 1}").expect_err("no geometry");
        assert!(matches!(not_a_geometry, GeometryError::MissingType), "{not_a_geometry:?}");
    }
}
