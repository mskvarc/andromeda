//! Reading a compact `GeoJSON` geometry object already parsed into a JSON
//! tree into a [`GeometryForm`].
//!
//! The tree's members are read into what they say, exactly as a geometry read
//! straight from a deserializer (see [`crate::written_geometry`]) reads them,
//! and the geometry is built from them by the one reading both share: the
//! `type` member selects how `coordinates` nests (IETF RFC 7946 sections 3.1.2
//! through 3.1.7), and each position, line and ring is built through the same
//! constructors every other geometry path uses, refused at the same place with
//! the same reason.
//!
//! A value that is not a `GeoJSON` geometry object at all -- an unknown
//! `type`, a missing or misshapen `coordinates`, an ordinate that is not a
//! number, a `bbox` that is not an array of numbers -- is refused with the
//! [`GeometryError`] naming what is missing or misplaced, and never with any
//! part of the value itself. The form is the positions alone: the members RFC
//! 7946 section 6.1 calls foreign are passed over, and a `bbox` is checked but
//! not kept; [`crate::geometry::Geometry::from_geojson_value`] keeps both.

use crate::{
    form::GeometryForm,
    geojson::GeometryError,
    geometry_members::{ForeignMemberReading, GeometryMembers},
};
use serde_json::Value;

impl GeometryForm {
    /// Read a compact `GeoJSON` geometry object.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NotAnObject`], [`GeometryError::MissingType`],
    /// [`GeometryError::UnknownType`], [`GeometryError::MissingCoordinates`],
    /// [`GeometryError::MissingGeometries`], [`GeometryError::InvalidBbox`],
    /// [`GeometryError::NotAnArray`] or [`GeometryError::NonNumericOrdinate`]
    /// for a value that is not a `GeoJSON` geometry object,
    /// [`GeometryError::GeometryCollectionUnsupported`]
    /// for a `GeometryCollection`, [`GeometryError::Position`] for a position
    /// RFC 7946 section 3.1.1 or section 4 gives no meaning,
    /// [`GeometryError::LineString`] for a `LineString` of fewer than two
    /// positions, and [`GeometryError::LinearRing`] for a Polygon ring that is
    /// open or holds fewer than four.
    pub fn from_compact_value(value: &Value) -> Result<GeometryForm, GeometryError> {
        GeometryMembers::of_object(value.as_object().ok_or(GeometryError::NotAnObject)?, ForeignMemberReading::Skipped).read()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        coordinate_path::CoordinatePath,
        form::GeometryForm,
        geojson::{CoordinateShape, GeometryError},
        geojson_oracle::{OracleRefusal, through_geojson},
        position::PositionError,
    };
    use serde_json::{Value, json};

    /// Whether `refusal` says the value is no `GeoJSON` geometry object, as
    /// opposed to a geometry the domain model refuses.
    const fn refuses_the_shape(refusal: &GeometryError) -> bool {
        matches!(
            refusal,
            GeometryError::NotAnObject
                | GeometryError::MissingType
                | GeometryError::UnknownType(_)
                | GeometryError::MissingCoordinates
                | GeometryError::MissingGeometries
                | GeometryError::InvalidBbox(_)
                | GeometryError::NotAnArray { .. }
                | GeometryError::NonNumericOrdinate { .. }
        )
    }

    /// Geometries every refusal and acceptance of both readings is compared
    /// over.
    fn cases() -> Vec<Value> {
        vec![
            json!({"type": "Point", "coordinates": [1.0, 2.0]}),
            json!({"type": "Point", "coordinates": [1.0, 2.0, 3.5]}),
            json!({"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}),
            json!({"type": "Point", "coordinates": [1.0]}),
            json!({"type": "Point", "coordinates": [181.0, 0.0]}),
            json!({"type": "Point", "coordinates": [0.0, 91.0]}),
            json!({"type": "MultiPoint", "coordinates": [[1.0, 2.0], [3.0, 4.0, 5.0]]}),
            json!({"type": "MultiPoint", "coordinates": []}),
            json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
            json!({"type": "LineString", "coordinates": [[1.0, 2.0]]}),
            json!({"type": "LineString", "coordinates": [[1.0], [2.0, 3.0]]}),
            json!({"type": "MultiLineString", "coordinates": [[[0.0, 0.0], [1.0, 1.0]], []]}),
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 0.0]]]}),
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]], [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]]]}),
            json!({"type": "Polygon", "coordinates": []}),
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [0.0, 95.0]]]}),
            json!({"type": "MultiPolygon", "coordinates": [[[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]], [[[5.0, 5.0], [6.0, 5.0], [5.0, 5.0]]]]}),
            json!({"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]], "bbox": [0.0, 0.0, 1.0, 1.0], "foreign": {"x": 1}}),
        ]
    }

    /// Both readings accept the same geometries into the same form, and
    /// refuse the others with the same structural reason at the same place.
    #[test]
    fn the_direct_reading_agrees_with_the_geojson_crate() {
        for case in cases() {
            match (GeometryForm::from_compact_value(&case), through_geojson(&case)) {
                (Ok(direct), Ok(oracle)) => pretty_assertions::assert_eq!(direct, oracle, "{case}"),
                // The geojson crate refuses a lone Point position of fewer than
                // two members before reading it; the direct reading names the
                // ordinate count, as it does for every other position.
                (
                    Err(GeometryError::Position {
                        source: PositionError::TooFewOrdinates { .. },
                        ..
                    }),
                    Err(OracleRefusal::NotGeojson(_)),
                ) => {}
                (Err(direct), Err(OracleRefusal::NotGeojson(_))) => assert!(refuses_the_shape(&direct), "{case}: {direct:?}"),
                (Err(direct), Err(OracleRefusal::Geometry(oracle))) => pretty_assertions::assert_eq!(direct.to_string(), oracle.to_string(), "{case}"),
                (direct, oracle) => panic!("{case}: the direct reading gave {direct:?}, the geojson crate {oracle:?}"),
            }
        }
    }

    #[test]
    fn a_value_that_is_not_a_geometry_object_is_refused_for_its_shape() {
        for value in [
            json!("text"),
            json!({"coordinates": [1.0, 2.0]}),
            json!({"type": "Nonsense", "coordinates": []}),
            json!({"type": "Point"}),
            json!({"type": "Point", "coordinates": "1,2"}),
            json!({"type": "Point", "coordinates": ["1", "2"]}),
            json!({"type": "LineString", "coordinates": [1.0, 2.0]}),
            json!({"type": "Point", "coordinates": [1.0, 2.0], "bbox": "box"}),
            json!({"type": "GeometryCollection"}),
        ] {
            let refused = GeometryForm::from_compact_value(&value).expect_err("not a geometry");
            assert!(refuses_the_shape(&refused), "{value}: {refused:?}");
            assert!(through_geojson(&value).is_err(), "the geojson crate refuses {value} as well");
        }
    }

    /// ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes `GeometryCollection`.
    #[test]
    fn a_geometry_collection_is_refused_as_a_geoproperty_geometry() {
        let collection = json!({"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [1.0, 2.0]}]});
        assert!(matches!(
            GeometryForm::from_compact_value(&collection),
            Err(GeometryError::GeometryCollectionUnsupported)
        ));
    }

    /// An arbitrary-precision decimal reads as the double nearest to it.
    #[test]
    fn an_arbitrary_precision_ordinate_reads_as_its_nearest_double() {
        let value: Value = serde_json::from_str(r#"{"type": "Point", "coordinates": [14.550000000000000001, 46.05]}"#).expect("valid JSON");
        let GeometryForm::Point(point) = GeometryForm::from_compact_value(&value).expect("a point") else {
            panic!("a point reads as a point");
        };
        pretty_assertions::assert_eq!(point.longitude().to_bits(), 14.55_f64.to_bits());
    }

    #[test]
    fn a_position_off_the_axes_names_its_place() {
        let refused = GeometryForm::from_compact_value(&json!({"type": "MultiPoint", "coordinates": [[0.0, 0.0], [0.0, 95.0]]})).expect_err("latitude 95");
        assert!(
            matches!(
                refused,
                GeometryError::Position {
                    source: PositionError::LatitudeOutOfRange { .. },
                    ..
                }
            ),
            "{refused:?}"
        );
        pretty_assertions::assert_eq!(
            refused.to_string(),
            "GeoJSON position latitude 95 is outside [-90, 90]; RFC 7946 section 4 places positions in WGS 84 longitude and latitude, in decimal degrees (at coordinates[1])"
        );
    }

    /// A misplaced member is refused with where it sits and what belongs
    /// there, and with no part of the value.
    #[test]
    fn a_misnested_member_names_its_place_and_the_shape_it_breaks() {
        let refused =
            GeometryForm::from_compact_value(&json!({"type": "MultiPoint", "coordinates": [[0.0, 0.0], "secret"]})).expect_err("a string is no position");
        assert!(
            matches!(refused, GeometryError::NotAnArray { expected: CoordinateShape::Position, ref at } if *at == CoordinatePath::new(vec![1])),
            "{refused:?}"
        );
        pretty_assertions::assert_eq!(
            refused.to_string(),
            "GeoJSON coordinates hold a non-array where a position belongs (at coordinates[1])"
        );
        let refused = GeometryForm::from_compact_value(&json!({"type": "Point", "coordinates": [0.0, "secret"]})).expect_err("a string is no ordinate");
        pretty_assertions::assert_eq!(refused.to_string(), "a GeoJSON position holds a member that is not a number (at coordinates)");
    }
}
