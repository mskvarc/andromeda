//! How many ordinates a geometry's positions carry.
//!
//! IETF RFC 7946 section 3.1.1 makes the third ordinate optional per position,
//! so "is this geometry three-dimensional" is a property of the whole geometry
//! rather than of its type. A geometry whose positions disagree is reported as
//! two-dimensional: the altitudes it does carry cover only part of it, and no
//! predicate may act on a height that some of the geometry does not have.

/// The dimensionality of the positions making up a geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeometryDimension {
    /// Longitude and latitude only.
    TwoD,
    /// Longitude, latitude and altitude, on every position of the geometry.
    ThreeD,
}
