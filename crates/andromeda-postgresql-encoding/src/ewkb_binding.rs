//! Binding geometries into PostgreSQL statements.
//!
//! `andromeda-geometry` deliberately declares no `sqlx` dependency, so
//! [`Ewkb`](crate::ewkb::Ewkb) implements none of sqlx's traits.
//! Every geometry bind goes through this module instead, which hands sqlx the
//! raw buffer as `bytea`; the statement's `ST_GeomFromEWKB($n)` projects it to
//! `PostGIS` `geometry` on the server. The SRID is already embedded, so no
//! `ST_SetSRID` wrap is needed, and binding as `bytea` avoids a runtime
//! `pg_type` OID lookup.

use crate::ewkb::{Ewkb, EwkbError, encode};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, great_circle_reach::GreatCircleReach, planar::PlanarGeometry};

/// Encode an absent-or-present geometry into the buffer a `bytea` bind takes.
///
/// Owned rather than borrowed because sqlx's builders hold their arguments
/// until the statement executes, which outlives any temporary the encode would
/// otherwise produce.
///
/// # Errors
///
/// Returns [`EwkbError`] if the geometry cannot be encoded.
pub fn geometry_bytes(geometry: Option<&GeometryForm>) -> Result<Option<Vec<u8>>, EwkbError> {
    geometry.map(|geometry| encode(geometry).map(|ewkb| ewkb.to_vec())).transpose()
}

/// The buffer a `bytea` bind of a geometry's great-circle reach takes, or
/// `None` for an absent geometry and for one whose reach stays within the
/// tolerance of its planar bounding rectangle.
#[must_use]
pub fn reach_bytes(geometry: Option<&GeometryForm>) -> Option<Vec<u8>> {
    geometry
        .and_then(|geometry| GreatCircleReach::of(&PlanarGeometry::project(geometry)))
        .map(|reach| Ewkb::from(&reach).to_vec())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        ewkb::Ewkb,
        ewkb_binding::{geometry_bytes, reach_bytes},
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, geometry::Geometry, position::Position};
    use pretty_assertions::assert_eq;

    #[test]
    fn an_absent_geometry_binds_as_null() {
        assert_eq!(geometry_bytes(None).expect("no geometry to encode"), None);
    }

    #[test]
    fn a_present_geometry_binds_as_its_ewkb_buffer() {
        let geometry =
            GeometryForm::from_compact_text(&serde_json::json!({ "type": "Point", "coordinates": [13.38, 52.52] }).to_string()).expect("point parses");
        let bytes = geometry_bytes(Some(&geometry)).expect("encodes").expect("present");
        assert_eq!(bytes.first(), Some(&0x01), "PostGIS reads little-endian EWKB: {bytes:?}");
    }

    #[test]
    fn a_point_binds_no_reach() {
        let point = GeometryForm::from_compact_text(&serde_json::json!({ "type": "Point", "coordinates": [13.38, 52.52] }).to_string()).expect("point parses");
        assert_eq!(reach_bytes(Some(&point)), None);
        assert_eq!(reach_bytes(None), None);
    }

    #[test]
    fn a_bowing_edge_binds_its_reach_as_a_polygon_that_decodes() {
        let line = GeometryForm::from_compact_text(&serde_json::json!({ "type": "LineString", "coordinates": [[-30.0, 60.0], [30.0, 60.0]] }).to_string())
            .expect("line parses");
        let bytes = reach_bytes(Some(&line)).expect("a long edge bows");
        let decoded = Geometry::try_from(&Ewkb::try_from(bytes.as_slice()).expect("canonical EWKB")).expect("decodes");
        let GeometryForm::Polygon(rings) = decoded.form() else {
            panic!("the reach is a polygon: {decoded:?}");
        };
        let positions = rings.rings().first().expect("one ring").positions();
        assert_eq!(positions.len(), 5);
        let north = positions.iter().map(Position::latitude).fold(f64::NEG_INFINITY, f64::max);
        assert!((north - 63.434_948_8).abs() < 1e-6, "north {north}");
        assert_eq!(positions.first(), positions.last());
    }
}
