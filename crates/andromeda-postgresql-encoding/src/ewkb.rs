//! `PostGIS` EWKB, the binary form geometries take on the way in and out of the
//! database.
//!
//! NGSI-LD says nothing about EWKB: ETSI GS CIM 009 v1.9.1 specifies
//! `GeoProperty` values as `GeoJSON` per RFC 7946, and that is what
//! [`Geometry`] models. EWKB is what `PostGIS` accepts natively
//! on its binary protocol, so the encode and decode happen here and the store
//! binds the buffer as `bytea`.
//!
//! The dialect is narrow and enforced: little-endian, XY or XYZ, never M, SRID
//! 4326. Z is carried because RFC 7946 section 3.1.1 makes altitude an optional
//! third ordinate of every position and `PostGIS` stores it natively; M has no
//! meaning in `GeoJSON` at all, so a buffer carrying one is refused rather than
//! silently flattened. Which of XY and XYZ a geometry encodes as is decided by
//! [`GeometryWalk`], per geometry rather than per position, because a WKB type
//! word describes the whole geometry.
//!
//! An encode/decode round trip is the identity: every Polygon ring a
//! [`Geometry`] holds is closed, as RFC 7946 section 3.1.6 requires, so the
//! OGC Simple Features polygon `PostGIS` stores is the one the client sent.
//!
//! There are deliberately no `sqlx` impls on [`Ewkb`]. They would forward to
//! `&[u8]` anyway, and keeping them out means this crate carries no persistence
//! dependency at all.

use crate::{
    ewkb_build::{GeometryBuildError, GeometryBuilder},
    ewkb_walk::GeometryWalk,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use bytes::Bytes;
use geozero::{GeozeroGeometry, ToWkb, error::GeozeroError, wkb::Ewkb as GeozeroEwkb};
use thiserror::Error;

/// WGS84 / CRS84, the only spatial reference NGSI-LD geometries use.
const SRID_WGS84: i32 = 4326;

/// First byte of a little-endian WKB or EWKB buffer.
const BYTE_ORDER_LITTLE_ENDIAN: u8 = 0x01;

/// Bit set in the EWKB type word when a SRID follows it.
const EWKB_SRID_FLAG: u32 = 0x2000_0000;
/// Bit set in the EWKB type word when M coordinates are present.
const EWKB_M_FLAG: u32 = 0x4000_0000;

/// Smallest buffer that can carry a byte-order marker, a type word and a SRID.
const MIN_EWKB_LEN: usize = 9;

/// Errors produced by [`Ewkb`] conversions.
#[derive(Debug, Error)]
pub enum EwkbError {
    /// The geozero codec failed to encode or decode the buffer.
    #[error("EWKB codec error: {0}")]
    Codec(#[from] GeozeroError),
    /// The decoded callbacks did not add up to a geometry.
    #[error("EWKB buffer does not describe a geometry: {0}")]
    Build(#[from] GeometryBuildError),
    /// The buffer is too short to be EWKB at all.
    #[error("EWKB buffer is {length} bytes, shorter than the {MIN_EWKB_LEN}-byte minimum")]
    TooShort {
        /// Length of the rejected buffer.
        length: usize,
    },
    /// The buffer is big-endian. `PostGIS` emits little-endian and the canonical
    /// dialect here is little-endian only.
    #[error("EWKB buffer is not little-endian (byte order marker {marker:#04x})")]
    NotLittleEndian {
        /// The byte-order marker actually found.
        marker: u8,
    },
    /// The type word has no SRID flag, so the buffer is plain WKB.
    #[error("buffer is WKB without an embedded SRID, not EWKB")]
    MissingSrid,
    /// The embedded SRID is not WGS84.
    #[error("EWKB buffer carries SRID {srid}, expected {SRID_WGS84}")]
    UnexpectedSrid {
        /// The SRID actually embedded in the buffer.
        srid: i32,
    },
    /// The geometry carries M (measure) coordinates, which `GeoJSON` has no
    /// place for and the domain model therefore cannot preserve.
    #[error("EWKB geometry carries unsupported M coordinates")]
    MeasuredCoordinates,
}

/// Encode a geometry on its way to a SQL bind.
///
/// A named function rather than a bare `Ewkb::try_from` so every bind site in
/// the store reads the same way.
///
/// # Errors
///
/// Returns [`EwkbError`] if the geometry cannot be encoded.
pub fn encode(geometry: &Geometry) -> Result<Ewkb, EwkbError> {
    Ewkb::try_from(geometry)
}

/// A geometry in `PostGIS` EWKB: little-endian, XY or XYZ, never M, SRID 4326.
///
/// Constructed only by encoding a [`Geometry`] or by validating bytes that came
/// back from a `geometry` column, so the dialect is an enforced invariant
/// rather than a documented assumption.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ewkb(Bytes);

impl Ewkb {
    /// Borrow the raw EWKB buffer.
    ///
    /// This is what the store binds: PostgreSQL `bytea`, projected to `PostGIS`
    /// `geometry` by `ST_GeomFromEWKB($n)`. The SRID is already embedded, so no
    /// `ST_SetSRID` wrap is needed, and binding as `bytea` avoids a runtime
    /// `pg_type` OID lookup.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    /// Copy the buffer into an owned vector.
    ///
    /// Needed where a builder demands ownership and cannot borrow, notably
    /// sea-query's `Value::Bytes`, which is `Vec<u8>`.
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl TryFrom<&Geometry> for Ewkb {
    type Error = EwkbError;

    fn try_from(geometry: &Geometry) -> Result<Ewkb, EwkbError> {
        let walk = GeometryWalk(geometry);
        let bytes = walk.to_ewkb(walk.dims(), Some(SRID_WGS84))?;
        Ok(Ewkb(Bytes::from(bytes)))
    }
}

impl TryFrom<&Ewkb> for Geometry {
    type Error = EwkbError;

    fn try_from(ewkb: &Ewkb) -> Result<Geometry, EwkbError> {
        let mut builder = GeometryBuilder::new();
        GeozeroEwkb(ewkb.0.as_ref()).process_geom(&mut builder)?;
        Ok(Geometry::new(builder.finish()?))
    }
}

impl TryFrom<&[u8]> for Ewkb {
    type Error = EwkbError;

    /// Validate that `bytes` really are canonical-dialect EWKB.
    ///
    /// `PostGIS` returns `geometry` columns in this exact form, but a `bytea`
    /// column can hold anything, so the dialect is checked rather than assumed.
    fn try_from(bytes: &[u8]) -> Result<Ewkb, EwkbError> {
        let Some(header) = bytes.get(..MIN_EWKB_LEN) else {
            return Err(EwkbError::TooShort { length: bytes.len() });
        };
        let marker = header[0];
        if marker != BYTE_ORDER_LITTLE_ENDIAN {
            return Err(EwkbError::NotLittleEndian { marker });
        }
        let type_word = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
        if type_word & EWKB_SRID_FLAG == 0 {
            return Err(EwkbError::MissingSrid);
        }
        // Z is accepted and carried; M is not, and a ZM buffer is refused for
        // the M it carries rather than for the Z it also carries.
        if type_word & EWKB_M_FLAG != 0 {
            return Err(EwkbError::MeasuredCoordinates);
        }
        let srid = i32::from_le_bytes([header[5], header[6], header[7], header[8]]);
        if srid != SRID_WGS84 {
            return Err(EwkbError::UnexpectedSrid { srid });
        }
        Ok(Ewkb(Bytes::copy_from_slice(bytes)))
    }
}

impl TryFrom<Bytes> for Ewkb {
    type Error = EwkbError;

    /// Validating constructor that keeps an already-owned buffer, avoiding the
    /// copy `TryFrom<&[u8]>` makes.
    fn try_from(bytes: Bytes) -> Result<Ewkb, EwkbError> {
        Ewkb::try_from(bytes.as_ref()).map(|_| Ewkb(bytes))
    }
}

#[cfg(test)]
mod tests {
    use crate::ewkb::{BYTE_ORDER_LITTLE_ENDIAN, EWKB_M_FLAG, EWKB_SRID_FLAG, Ewkb, EwkbError, SRID_WGS84};
    use andromeda_ngsi_ld_geometry::{
        dimension::GeometryDimension,
        form::GeometryForm,
        geometry::Geometry,
        line_string_coordinates::LineStringCoordinates,
        position::Position,
    };
    use serde_json::{Value, json};

    /// Bit set in the EWKB type word when Z coordinates are present.
    const EWKB_Z_FLAG: u32 = 0x8000_0000;

    fn parse(value: &Value) -> Geometry {
        Geometry::from_geojson_value(value).expect("valid GeoJSON")
    }

    fn point() -> Geometry {
        parse(&json!({ "type": "Point", "coordinates": [13.38, 52.52] }))
    }

    fn elevated_point() -> Geometry {
        parse(&json!({ "type": "Point", "coordinates": [13.38, 52.52, 34.5] }))
    }

    fn polygon() -> Geometry {
        parse(&json!({
            "type": "Polygon",
            "coordinates": [[[1.0, 2.0], [5.0, 2.0], [5.0, 8.0], [1.0, 8.0], [1.0, 2.0]]]
        }))
    }

    fn type_word(ewkb: &Ewkb) -> u32 {
        let bytes = ewkb.as_bytes();
        u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]])
    }

    fn two_dimensional_cases() -> Vec<Geometry> {
        [
            json!({ "type": "Point", "coordinates": [13.38, 52.52] }),
            json!({ "type": "LineString", "coordinates": [[1.0, 2.0], [3.0, 4.0]] }),
            json!({ "type": "Polygon", "coordinates": [[[1.0, 2.0], [5.0, 2.0], [5.0, 8.0], [1.0, 8.0], [1.0, 2.0]]] }),
            json!({ "type": "MultiPoint", "coordinates": [[1.0, 2.0], [3.0, 4.0]] }),
            json!({ "type": "MultiLineString", "coordinates": [[[1.0, 2.0], [3.0, 4.0]], [[5.0, 6.0], [7.0, 8.0]]] }),
            json!({ "type": "MultiPolygon", "coordinates": [[[[1.0, 2.0], [5.0, 2.0], [5.0, 8.0], [1.0, 8.0], [1.0, 2.0]]]] }),
        ]
        .iter()
        .map(parse)
        .collect()
    }

    fn three_dimensional_cases() -> Vec<Geometry> {
        [
            json!({ "type": "Point", "coordinates": [13.38, 52.52, 34.5] }),
            json!({ "type": "LineString", "coordinates": [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] }),
            json!({ "type": "Polygon", "coordinates": [[[1.0, 2.0, 9.0], [5.0, 2.0, 9.0], [5.0, 8.0, 9.0], [1.0, 8.0, 9.0], [1.0, 2.0, 9.0]]] }),
            json!({ "type": "MultiPoint", "coordinates": [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]] }),
            json!({ "type": "MultiLineString", "coordinates": [[[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]], [[7.0, 8.0, 9.0], [10.0, 11.0, 12.0]]] }),
            json!({ "type": "MultiPolygon", "coordinates": [[[[1.0, 2.0, 9.0], [5.0, 2.0, 9.0], [5.0, 8.0, 9.0], [1.0, 8.0, 9.0], [1.0, 2.0, 9.0]]]] }),
        ]
        .iter()
        .map(parse)
        .collect()
    }

    #[test]
    fn a_geometry_encodes_to_little_endian_ewkb_with_srid_4326() {
        let ewkb = Ewkb::try_from(&point()).expect("encodes");
        let bytes = ewkb.as_bytes();
        pretty_assertions::assert_eq!(bytes[0], BYTE_ORDER_LITTLE_ENDIAN);
        assert!(type_word(&ewkb) & EWKB_SRID_FLAG != 0, "SRID flag must be set in {:#010x}", type_word(&ewkb));
        pretty_assertions::assert_eq!(i32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]), SRID_WGS84);
    }

    #[test]
    fn ewkb_decodes_back_to_the_originating_geometry() {
        let original = point();
        let ewkb = Ewkb::try_from(&original).expect("encodes");
        pretty_assertions::assert_eq!(Geometry::try_from(&ewkb).expect("decodes"), original);
    }

    #[test]
    fn a_polygon_survives_the_full_encode_decode_cycle() {
        let original = polygon();
        let ewkb = Ewkb::try_from(&original).expect("encodes");
        pretty_assertions::assert_eq!(Geometry::try_from(&ewkb).expect("decodes"), original);
    }

    #[test]
    fn every_supported_two_dimensional_geometry_round_trips_through_ewkb() {
        for original in two_dimensional_cases() {
            let ewkb = Ewkb::try_from(&original).expect("encodes");
            pretty_assertions::assert_eq!(type_word(&ewkb) & EWKB_Z_FLAG, 0, "a planar geometry sets no Z flag");
            pretty_assertions::assert_eq!(Geometry::try_from(&ewkb).expect("decodes"), original);
        }
    }

    #[test]
    fn every_supported_three_dimensional_geometry_round_trips_through_ewkb() {
        for original in three_dimensional_cases() {
            pretty_assertions::assert_eq!(original.dimension(), GeometryDimension::ThreeD);
            let ewkb = Ewkb::try_from(&original).expect("encodes");
            let decoded = Geometry::try_from(&ewkb).expect("decodes");
            pretty_assertions::assert_eq!(decoded, original);
            pretty_assertions::assert_eq!(decoded.dimension(), GeometryDimension::ThreeD);
        }
    }

    #[test]
    fn a_geometry_collection_buffer_is_refused_rather_than_decoded() {
        // ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes the GeometryCollection
        // from GeoProperty geometries, so a column holding one is a row no write
        // of this broker produced. A hand-built EWKB buffer stands in for it,
        // because the encoder can no longer produce one: SRID-flagged type word
        // 7, SRID 4326, one member Point at (1, 2).
        let mut bytes = vec![BYTE_ORDER_LITTLE_ENDIAN];
        bytes.extend_from_slice(&(7_u32 | EWKB_SRID_FLAG).to_le_bytes());
        bytes.extend_from_slice(&SRID_WGS84.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(BYTE_ORDER_LITTLE_ENDIAN);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
        bytes.extend_from_slice(&2.0_f64.to_le_bytes());
        let bytes = bytes;

        let ewkb = Ewkb::try_from(bytes.as_slice()).expect("the dialect header is canonical");
        let error = Geometry::try_from(&ewkb).expect_err("a collection is not a geometry the broker stores");
        assert!(
            error
                .to_string()
                .contains("GeometryCollection, which ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes"),
            "got {error}"
        );
    }

    #[test]
    fn a_three_dimensional_point_encodes_with_the_z_and_srid_flags_set() {
        let ewkb = Ewkb::try_from(&elevated_point()).expect("encodes");
        let word = type_word(&ewkb);
        assert!(word & EWKB_Z_FLAG != 0, "the Z flag must be set in {word:#010x}");
        assert!(word & EWKB_SRID_FLAG != 0, "the SRID flag must be set in {word:#010x}");
        assert!(word & EWKB_M_FLAG == 0, "no M flag may be set in {word:#010x}");
    }

    #[test]
    fn a_three_dimensional_point_decodes_back_to_the_altitude_it_carried() {
        let ewkb = Ewkb::try_from(&elevated_point()).expect("encodes");
        let decoded = Geometry::try_from(&ewkb).expect("decodes");
        pretty_assertions::assert_eq!(
            decoded.form(),
            &GeometryForm::Point(Position::try_new(13.38, 52.52, Some(34.5)).expect("a test position lies on the CRS84 axes"))
        );
    }

    #[test]
    fn a_two_dimensional_buffer_decodes_to_positions_without_an_altitude() {
        let ewkb = Ewkb::try_from(&point()).expect("encodes");
        let decoded = Geometry::try_from(&ewkb).expect("decodes");
        pretty_assertions::assert_eq!(
            decoded.form(),
            &GeometryForm::Point(Position::try_new(13.38, 52.52, None).expect("a test position lies on the CRS84 axes"))
        );
        pretty_assertions::assert_eq!(decoded.dimension(), GeometryDimension::TwoD);
    }

    #[test]
    fn a_geometry_of_mixed_position_arity_encodes_as_two_dimensional() {
        let original = parse(&json!({ "type": "LineString", "coordinates": [[1.0, 2.0, 3.0], [4.0, 5.0]] }));
        let ewkb = Ewkb::try_from(&original).expect("encodes");
        pretty_assertions::assert_eq!(type_word(&ewkb) & EWKB_Z_FLAG, 0, "a partly elevated geometry sets no Z flag");
        let decoded = Geometry::try_from(&ewkb).expect("decodes");
        pretty_assertions::assert_eq!(
            decoded.form(),
            &GeometryForm::LineString(
                LineStringCoordinates::try_new(vec![
                    Position::try_new(1.0, 2.0, None).expect("a test position lies on the CRS84 axes"),
                    Position::try_new(4.0, 5.0, None).expect("a test position lies on the CRS84 axes")
                ])
                .expect("two positions trace a line")
            )
        );
    }

    #[test]
    fn a_polygon_with_a_hole_round_trips_ring_for_ring() {
        let original = parse(&json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0], [0.0, 0.0]],
                [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 1.0]]
            ]
        }));
        let ewkb = Ewkb::try_from(&original).expect("encodes");
        pretty_assertions::assert_eq!(Geometry::try_from(&ewkb).expect("decodes"), original);
    }

    #[test]
    fn encoded_bytes_pass_the_validating_constructor() {
        let ewkb = Ewkb::try_from(&point()).expect("encodes");
        pretty_assertions::assert_eq!(Ewkb::try_from(ewkb.as_bytes()).expect("revalidates"), ewkb);
    }

    #[test]
    fn a_three_dimensional_buffer_passes_the_validating_constructor() {
        let ewkb = Ewkb::try_from(&elevated_point()).expect("encodes");
        pretty_assertions::assert_eq!(Ewkb::try_from(ewkb.as_bytes()).expect("Z is part of the dialect"), ewkb);
    }

    #[test]
    fn non_ewkb_bytes_are_rejected_rather_than_wrapped() {
        let error = Ewkb::try_from(&[0xFF, 0x00, 0x01][..]).expect_err("must reject");
        assert!(matches!(error, EwkbError::TooShort { length: 3 }), "got {error:?}");
    }

    #[test]
    fn big_endian_input_is_rejected() {
        let mut bytes = Ewkb::try_from(&point()).expect("encodes").to_vec();
        bytes[0] = 0x00;
        let error = Ewkb::try_from(bytes.as_slice()).expect_err("must reject");
        assert!(matches!(error, EwkbError::NotLittleEndian { marker: 0x00 }), "got {error:?}");
    }

    #[test]
    fn plain_wkb_without_a_srid_is_rejected() {
        let mut bytes = Ewkb::try_from(&point()).expect("encodes").to_vec();
        let word = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) & !EWKB_SRID_FLAG;
        bytes[1..5].copy_from_slice(&word.to_le_bytes());
        let error = Ewkb::try_from(bytes.as_slice()).expect_err("must reject");
        assert!(matches!(error, EwkbError::MissingSrid), "got {error:?}");
    }

    #[test]
    fn a_foreign_srid_is_rejected() {
        let mut bytes = Ewkb::try_from(&point()).expect("encodes").to_vec();
        bytes[5..9].copy_from_slice(&3857_i32.to_le_bytes());
        let error = Ewkb::try_from(bytes.as_slice()).expect_err("must reject");
        assert!(matches!(error, EwkbError::UnexpectedSrid { srid: 3857 }), "got {error:?}");
    }

    #[test]
    fn m_and_zm_ewkb_are_rejected_before_geozero_decoding() {
        for flag in [EWKB_M_FLAG, EWKB_Z_FLAG | EWKB_M_FLAG] {
            let mut bytes = Ewkb::try_from(&point()).expect("encodes").to_vec();
            let word = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) | flag;
            bytes[1..5].copy_from_slice(&word.to_le_bytes());
            let error = Ewkb::try_from(bytes.as_slice()).expect_err("must reject");
            assert!(matches!(error, EwkbError::MeasuredCoordinates), "got {error:?} for flag {flag:#010x}");
        }
    }
}
