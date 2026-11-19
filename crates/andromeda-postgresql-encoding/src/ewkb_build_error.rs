//! The failures of assembling a domain geometry from a geozero reader's callbacks.
//!
//! A `GeomProcessor` callback may only fail as a `GeozeroError`, whose payload
//! is a string, so the typed failure is rendered into one at that boundary and
//! read back as text by whoever ran the reader.

use andromeda_ngsi_ld_geometry::{
    geometry_type::GeometryType,
    line_string_coordinates::LineStringError,
    linear_ring::LinearRingError,
    position::PositionError,
};
use geozero::error::GeozeroError;
use strum::Display;
use thiserror::Error;

/// Failures assembling a geometry from a reader's callbacks.
///
/// Every variant describes a callback stream that no well-formed EWKB buffer
/// produces, so each one means the buffer was malformed or held a geometry
/// outside the `GeoJSON` model.
#[derive(Debug, Error, Clone, Copy, PartialEq)]
pub enum GeometryBuildError {
    /// A coordinate that is no RFC 7946 position: off the CRS84 axes of
    /// section 4, or with a non-finite altitude. Every geometry the broker
    /// writes was built from checked positions, so such a row was not written
    /// by the broker.
    #[error("the buffer holds a coordinate that is no GeoJSON position: {0}")]
    Position(#[from] PositionError),
    /// A line string of fewer than the two positions RFC 7946 section 3.1.4
    /// requires. Every line the broker writes was built checked, so such a row
    /// was not written by the broker.
    #[error("the buffer holds a line that is no GeoJSON LineString: {0}")]
    LineString(#[from] LineStringError),
    /// A polygon ring that is open or holds fewer than the four positions RFC
    /// 7946 section 3.1.6 requires, which no ring the broker writes is.
    #[error("the buffer holds a ring that is no GeoJSON linear ring: {0}")]
    LinearRing(#[from] LinearRingError),
    /// A coordinate arrived while no geometry that holds positions was open.
    #[error("a coordinate arrived outside any geometry that can hold one")]
    UnexpectedCoordinate,
    /// A Point received a second position; RFC 7946 section 3.1.2 gives it one.
    #[error("a Point carried a second position")]
    RepeatedPointCoordinate,
    /// A Point ended without the single position it must carry.
    #[error("a Point ended without a position")]
    MissingPointCoordinate,
    /// A geometry ended without the matching begin, so the callback stream is
    /// not properly nested.
    #[error("a {geometry} ended without a matching begin")]
    UnbalancedGeometry {
        /// The geometry whose end had no matching begin.
        geometry: GeometryType,
    },
    /// A geometry finished inside another that cannot contain it, such as a
    /// Polygon nested directly in a `MultiPoint`.
    #[error("a {geometry} ended inside a geometry that cannot contain one")]
    MisplacedGeometry {
        /// The geometry that finished in the wrong place.
        geometry: GeometryType,
    },
    /// A second top-level geometry followed the first, which no single EWKB
    /// buffer encodes.
    #[error("the buffer encoded more than one top-level geometry")]
    TrailingGeometry,
    /// The callback stream ended without producing a geometry.
    #[error("the buffer encoded no geometry")]
    MissingGeometry,
    /// The buffer holds a `PostGIS` geometry type that `GeoJSON`, and therefore
    /// ETSI GS CIM 009 v1.9.1 clause 4.7, has no representation for.
    #[error("the buffer encodes a {geometry}, which GeoJSON has no geometry type for")]
    UnsupportedGeometryType {
        /// The `PostGIS` geometry type that cannot be represented.
        geometry: PostgisOnlyGeometryType,
    },
    /// The buffer holds a `GeometryCollection`. `GeoJSON` defines one, but ETSI GS
    /// CIM 009 v1.9.1 clause 4.7.1 allows a `GeoProperty` every RFC 7946 geometry
    /// except that one, so no geometry the broker stores is a collection.
    #[error("the buffer encodes a GeometryCollection, which no GeoProperty holds")]
    GeometryCollectionUnsupported,
}

/// A `PostGIS` geometry type that `GeoJSON` has no counterpart for: the SQL/MM
/// curves and the polyhedral surfaces.
///
/// The variant names are the `PostGIS` type names, which the error renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum PostgisOnlyGeometryType {
    /// A line of circular arcs.
    CircularString,
    /// A line of straight and circular segments.
    CompoundCurve,
    /// A polygon whose rings may be curves.
    CurvePolygon,
    /// A collection of curves.
    MultiCurve,
    /// A collection of surfaces, curved polygons among them.
    MultiSurface,
    /// A polygon of exactly three distinct vertices.
    Triangle,
    /// A surface of polygons sharing edges.
    PolyhedralSurface,
    /// A triangulated irregular network.
    Tin,
}

// Hand-written because the trait is geozero's and the error it carries is a
// string: a `GeomProcessor` callback may only fail as `GeozeroError`, so a typed
// build failure has to be rendered into one at that boundary.
impl From<GeometryBuildError> for GeozeroError {
    fn from(error: GeometryBuildError) -> GeozeroError {
        GeozeroError::Geometry(error.to_string())
    }
}
