//! Geometry in the NGSI-LD data model.
//!
//! NGSI-LD (ETSI GS CIM 009 v1.9.1, clause 4.7 and Table 5.2.7-1) defines
//! GeoProperty values as GeoJSON geometries per IETF RFC 7946, in WGS84 /
//! CRS84. That is the representation this type serialises to and from. No
//! storage encoding appears here: PostGIS EWKB is a persistence concern and
//! lives with the store, in `crate::repository::geometry`.

use crate::domain::attribute::serde_json_value_to_json_value;
use derive_more::{AsRef, From};
use geo::algorithm::BoundingRect;
use geojson::{Geometry as GeoJsonGeometry, GeometryValue};
use jsonld_expandable_core::{JsonValue, ToJsonValue};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

/// Errors produced by [`Geometry`] conversions.
#[derive(Debug, Error)]
pub enum GeometryError {
    /// A syntactically valid GeoJSON geometry that `geo` cannot represent,
    /// for example a coordinate list of the wrong arity for its type.
    #[error(transparent)]
    Geojson(#[from] geojson::Error),
    /// The raw JSON value was not a GeoJSON geometry object at all.
    #[error("invalid GeoJSON geometry value: {0}")]
    GeojsonValue(#[from] serde_json::Error),
}

/// A geometry attached to an NGSI-LD GeoProperty (clause 5.2.7).
///
/// Two-dimensional and in WGS84 / CRS84, as RFC 7946 mandates for GeoJSON
/// without an explicit CRS member. Equality is structural over the
/// coordinates; there is deliberately no `Eq`/`Hash`, because the coordinates
/// are `f64` and no total order or hash over them would be meaningful.
#[derive(Debug, Clone, PartialEq, From, AsRef, Deserialize)]
#[serde(try_from = "GeoJsonGeometry")]
pub struct Geometry(geo::Geometry<f64>);

impl Geometry {
    /// Build from a parsed GeoJSON geometry.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::Geojson`] if the geometry cannot be expressed
    /// as a `geo::Geometry<f64>`.
    pub fn from_geojson(geometry: &GeoJsonGeometry) -> Result<Geometry, GeometryError> {
        Ok(Geometry(geometry.try_into()?))
    }

    /// Build from an arbitrary JSON value that is expected to encode a GeoJSON
    /// geometry object.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::GeojsonValue`] if `value` is not a GeoJSON
    /// geometry object, or [`GeometryError::Geojson`] if it is one that `geo`
    /// cannot represent.
    pub fn from_geojson_value(value: &Value) -> Result<Geometry, GeometryError> {
        let geometry: GeoJsonGeometry = serde_json::from_value(value.clone())?;
        Geometry::from_geojson(&geometry)
    }

    /// Render as a GeoJSON geometry for the wire.
    ///
    /// Infallible: every `geo::Geometry<f64>` has a GeoJSON form.
    pub fn to_geojson(&self) -> GeoJsonGeometry {
        GeoJsonGeometry::new(GeometryValue::from(&self.0))
    }

    /// Axis-aligned bounding rectangle, or `None` for a geometry with no
    /// coordinates (an empty collection).
    pub fn bounding_rect(&self) -> Option<geo::Rect<f64>> {
        self.0.bounding_rect()
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
        // inside this signature, which has no way to report a failure.
        let json = Value::from(&GeometryValue::from(&self.0));
        serde_json_value_to_json_value::<V>(&json)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use geojson::Position;

    fn point_json() -> Value {
        serde_json::json!({ "type": "Point", "coordinates": [13.38, 52.52] })
    }

    fn polygon_json() -> Value {
        serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0], [5.0, 2.0], [5.0, 8.0], [1.0, 8.0], [1.0, 2.0]]]
        })
    }

    #[test]
    fn a_geojson_point_round_trips_through_the_domain_type() {
        let geometry = Geometry::from_geojson_value(&point_json()).expect("point parses");
        let GeometryValue::Point { coordinates } = geometry.to_geojson().value else {
            unreachable!("expected a Point");
        };
        pretty_assertions::assert_eq!(coordinates.as_slice(), [13.38, 52.52]);
    }

    #[test]
    fn a_geojson_polygon_round_trips_through_the_domain_type() {
        let geometry = Geometry::from_geojson_value(&polygon_json()).expect("polygon parses");
        let GeometryValue::Polygon { coordinates } = geometry.to_geojson().value else {
            unreachable!("expected a Polygon");
        };
        let ring = coordinates.first().expect("exterior ring");
        pretty_assertions::assert_eq!(ring.len(), 5);
        pretty_assertions::assert_eq!(ring.first().map(Position::as_slice), Some(&[1.0, 2.0][..]));
        pretty_assertions::assert_eq!(ring.last().map(Position::as_slice), Some(&[1.0, 2.0][..]));
        pretty_assertions::assert_eq!(ring.get(2).map(Position::as_slice), Some(&[5.0, 8.0][..]));
    }

    #[test]
    fn a_malformed_geometry_value_is_a_typed_error() {
        let error = Geometry::from_geojson_value(&serde_json::json!({ "type": "Nonsense", "coordinates": [] })).expect_err("must reject");
        assert!(matches!(error, GeometryError::GeojsonValue(_)), "got {error:?}");
    }

    #[test]
    fn a_geometry_with_unrepresentable_coordinates_is_a_typed_error() {
        let error = Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [] })).expect_err("must reject");
        assert!(matches!(error, GeometryError::GeojsonValue(_)), "got {error:?}");
    }

    #[test]
    fn the_bounding_rectangle_of_a_polygon_covers_every_vertex() {
        let geometry = Geometry::from_geojson_value(&polygon_json()).expect("polygon parses");
        let rect = geometry.bounding_rect().expect("non-empty geometry has a bounding rect");
        pretty_assertions::assert_eq!(rect.min(), geo::Coord { x: 1.0, y: 2.0 });
        pretty_assertions::assert_eq!(rect.max(), geo::Coord { x: 5.0, y: 8.0 });
    }

    #[test]
    fn a_geometry_serialises_as_the_geojson_the_spec_requires() {
        let geometry = Geometry::from_geojson_value(&point_json()).expect("point parses");
        // RFC 7946 section 3.1, mandated for GeoProperty values by ETSI GS CIM
        // 009 v1.9.1 clause 4.7.
        pretty_assertions::assert_eq!(serde_json::to_value(&geometry).expect("serialises"), point_json());
    }

    #[test]
    fn geometry_serde_round_trips_without_loss() {
        let geometry = Geometry::from_geojson_value(&polygon_json()).expect("polygon parses");
        let encoded = serde_json::to_value(&geometry).expect("serialises");
        let decoded: Geometry = serde_json::from_value(encoded).expect("deserialises");
        pretty_assertions::assert_eq!(decoded, geometry);
    }

    #[test]
    fn a_geo_geometry_converts_in_and_out_without_a_named_constructor() {
        let source = geo::Geometry::Point(geo::Point::new(1.5, 2.5));
        let geometry = Geometry::from(source.clone());
        pretty_assertions::assert_eq!(AsRef::<geo::Geometry<f64>>::as_ref(&geometry), &source);
    }
}
