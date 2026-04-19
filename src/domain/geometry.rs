//! Canonical internal geometry representation.
//!
//! Andromeda carries geometries as EWKB (PostGIS Extended Well-Known Binary,
//! little-endian, SRID 4326) through every internal layer. This is the exact
//! wire form PostgreSQL/PostGIS expects for the `geometry` binary protocol, so
//! sqlx bind is a direct buffer write — no re-encoding. GeoJSON appears only
//! at the HTTP boundary and `geo::Geometry<f64>` is the ephemeral-compute form
//! decoded from EWKB on demand.
//!
//! Canonical dialect (EWKB, little-endian, XY, SRID 4326) means byte equality
//! implies semantic equality, so `Wkb` is `Eq` and `Hash`.

use bytes::Bytes;
use geo::algorithm::BoundingRect;
use geojson::{Geometry as GeoJsonGeometry, GeometryValue};
use geozero::{CoordDimensions, ToGeo, ToWkb, wkb::Ewkb};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;
use sqlx::{
    Postgres,
    encode::{Encode, IsNull},
    postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo},
};
use thiserror::Error;

/// Errors produced by [`Wkb`] conversions.
#[derive(Debug, Error)]
pub enum WkbError {
    /// Underlying geozero codec failure.
    #[error("WKB codec error: {0}")]
    Codec(#[from] geozero::error::GeozeroError),
    /// GeoJSON to `geo::Geometry` conversion failed.
    #[error("invalid GeoJSON geometry: {0}")]
    Geojson(String),
    /// Raw serde_json value was not a valid GeoJSON geometry.
    #[error("invalid GeoJSON geometry value: {0}")]
    GeojsonValue(#[from] serde_json::Error),
}

/// Canonical geometry bytes — EWKB, little-endian, XY, SRID 4326.
///
/// Bytewise equality implies semantic equality because every `Wkb` is produced
/// through the same canonical dialect.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Wkb(Bytes);

impl Wkb {
    /// Build from a `geo::Geometry<f64>` using the canonical EWKB dialect (SRID 4326).
    pub fn from_geo(geometry: &geo::Geometry<f64>) -> Result<Wkb, WkbError> {
        let bytes = geometry.to_ewkb(CoordDimensions::xy(), Some(4326))?;
        Ok(Wkb(Bytes::from(bytes)))
    }

    /// Build from a parsed `geojson::Geometry`.
    pub fn from_geojson(geometry: &GeoJsonGeometry) -> Result<Wkb, WkbError> {
        let geo_geom: geo::Geometry<f64> = geometry.try_into().map_err(|error: geojson::Error| WkbError::Geojson(error.to_string()))?;
        Wkb::from_geo(&geo_geom)
    }

    /// Build from an arbitrary `serde_json::Value` that encodes a GeoJSON geometry.
    pub fn from_geojson_value(value: &Value) -> Result<Wkb, WkbError> {
        let geometry: GeoJsonGeometry = serde_json::from_value(value.clone())?;
        Wkb::from_geojson(&geometry)
    }

    /// Decode to `geo::Geometry<f64>` for predicate/centroid/distance math.
    pub fn to_geo(&self) -> Result<geo::Geometry<f64>, WkbError> {
        let reader = Ewkb(self.0.as_ref());
        Ok(reader.to_geo()?)
    }

    /// Decode to `geojson::Geometry` for wire serialization.
    pub fn to_geojson(&self) -> Result<GeoJsonGeometry, WkbError> {
        let geo_geom = self.to_geo()?;
        Ok(GeoJsonGeometry::new(GeometryValue::from(&geo_geom)))
    }

    /// Compute the axis-aligned bounding rectangle without retaining the decoded form.
    pub fn bounding_rect(&self) -> Option<geo::Rect<f64>> {
        self.to_geo().ok().and_then(|geometry| geometry.bounding_rect())
    }

    /// Wrap raw EWKB bytes returned by PostGIS (`geometry::bytea`) as a `Wkb`.
    ///
    /// Caller is responsible for guaranteeing the bytes were produced by a
    /// canonical-dialect PostGIS `geometry` column (EWKB, SRID 4326).
    pub const fn from_postgis_bytes(bytes: Bytes) -> Wkb {
        Wkb(bytes)
    }

    /// Produce an owned EWKB byte vector (used for SQL bind construction).
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl Serialize for Wkb {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let geometry = self.to_geojson().map_err(|error| serde::ser::Error::custom(error.to_string()))?;
        geometry.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Wkb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Wkb, D::Error> {
        let geometry = GeoJsonGeometry::deserialize(deserializer)?;
        Wkb::from_geojson(&geometry).map_err(D::Error::custom)
    }
}

// `Wkb` binds as PostgreSQL `bytea` — the raw EWKB bytes. The repository SQL
// layer calls `ST_GeomFromEWKB($n)` to project `bytea` to PostGIS `geometry`
// (SRID already embedded, no `ST_SetSRID` wrap needed). Binding as `bytea`
// rather than `geometry` avoids a runtime `pg_type` OID lookup and matches the
// binary format PostGIS accepts natively from EWKB.
impl sqlx::Type<Postgres> for Wkb {
    fn type_info() -> PgTypeInfo {
        <&[u8] as sqlx::Type<Postgres>>::type_info()
    }

    fn compatible(ty: &PgTypeInfo) -> bool {
        <&[u8] as sqlx::Type<Postgres>>::compatible(ty)
    }
}

impl PgHasArrayType for Wkb {
    fn array_type_info() -> PgTypeInfo {
        <&[u8] as PgHasArrayType>::array_type_info()
    }
}

impl Encode<'_, Postgres> for Wkb {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, sqlx::error::BoxDynError> {
        <&[u8] as Encode<'_, Postgres>>::encode_by_ref(&self.0.as_ref(), buf)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn round_trip_point_geojson() {
        let input = serde_json::json!({ "type": "Point", "coordinates": [13.0, 52.0] });
        let wkb: Wkb = serde_json::from_value(input.clone()).expect("parse point");
        let back = serde_json::to_value(&wkb).expect("serialize");
        pretty_assertions::assert_eq!(back, input);
    }

    #[test]
    fn wkb_hash_eq_stable_for_same_input() {
        let input = serde_json::json!({ "type": "Point", "coordinates": [13.0, 52.0] });
        let a: Wkb = serde_json::from_value(input.clone()).expect("first");
        let b: Wkb = serde_json::from_value(input).expect("second");
        pretty_assertions::assert_eq!(a, b);
    }

    #[test]
    fn invalid_geojson_rejected_at_boundary() {
        let bad = serde_json::json!({ "type": "Point", "coordinates": [] });
        let result: Result<Wkb, _> = serde_json::from_value(bad);
        assert!(result.is_err());
    }

    #[test]
    fn bounding_rect_for_polygon() {
        let input = serde_json::json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0], [0.0, 0.0]]]
        });
        let wkb: Wkb = serde_json::from_value(input).expect("polygon");
        let rect = wkb.bounding_rect().expect("bbox");
        pretty_assertions::assert_eq!(rect.min(), geo::Coord { x: 0.0, y: 0.0 });
        pretty_assertions::assert_eq!(rect.max(), geo::Coord { x: 10.0, y: 5.0 });
    }

    #[test]
    fn canonical_dialect_is_ewkb_srid_4326() {
        let input = serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] });
        let wkb: Wkb = serde_json::from_value(input).expect("point");
        // EWKB carries SRID 4326 (0x10E6) little-endian after the type uint32.
        assert!(wkb.0.windows(4).any(|w| w == [0xE6, 0x10, 0x00, 0x00]), "SRID 4326 encoded little-endian");
    }
}
