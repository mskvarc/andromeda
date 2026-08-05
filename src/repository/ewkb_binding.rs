//! Binding geometries into PostgreSQL statements.
//!
//! `andromeda-geometry` deliberately declares no `sqlx` dependency, so
//! [`Ewkb`](andromeda_geometry::ewkb::Ewkb) implements none of sqlx's traits.
//! Every geometry bind goes through this module instead, which hands sqlx the
//! raw buffer as `bytea`; the statement's `ST_GeomFromEWKB($n)` projects it to
//! PostGIS `geometry` on the server. The SRID is already embedded, so no
//! `ST_SetSRID` wrap is needed, and binding as `bytea` avoids a runtime
//! `pg_type` OID lookup.

use crate::core::error::AndromedaError;
use andromeda_geometry::{ewkb::encode, geometry::Geometry};

/// Encode an absent-or-present geometry into the buffer a `bytea` bind takes.
///
/// Owned rather than borrowed because sqlx's builders hold their arguments
/// until the statement executes, which outlives any temporary the encode would
/// otherwise produce.
///
/// # Errors
///
/// Returns [`AndromedaError::internal`] if the geometry cannot be encoded.
pub fn geometry_bytes(geometry: Option<&Geometry>) -> Result<Option<Vec<u8>>, AndromedaError> {
    geometry
        .map(|geometry| encode(geometry).map(|ewkb| ewkb.to_vec()))
        .transpose()
        .map_err(AndromedaError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_geometry_binds_as_null() {
        assert_eq!(geometry_bytes(None).expect("no geometry to encode"), None);
    }

    #[test]
    fn a_present_geometry_binds_as_its_ewkb_buffer() {
        let geometry = Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [13.38, 52.52] })).expect("point parses");
        let bytes = geometry_bytes(Some(&geometry)).expect("encodes").expect("present");
        assert_eq!(bytes.first(), Some(&0x01), "PostGIS reads little-endian EWKB: {bytes:?}");
    }
}
