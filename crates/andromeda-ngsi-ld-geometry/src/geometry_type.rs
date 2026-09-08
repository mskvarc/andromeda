//! The `type` token of a `GeoJSON` geometry object, standing alone.
//!
//! ETSI GS CIM 009 v1.9.1 uses the bare token in places where no coordinates
//! accompany it — the `geometry` member of a GeoJSON-rendered query result
//! among them — so it is a value in its own right, distinct from
//! [`crate::form::GeometryForm`], which is the token together with the
//! coordinates it describes.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The `type` member of a `GeoJSON` geometry object (IETF RFC 7946 section 1.4).
///
/// The variant names are the RFC's tokens verbatim, which is also the casing
/// NGSI-LD carries them in (clause 4.7); no serde renaming applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum GeometryType {
    /// A single position.
    Point,
    /// An array of positions.
    MultiPoint,
    /// An array of two or more positions.
    LineString,
    /// An array of `LineString` coordinate arrays.
    MultiLineString,
    /// An array of linear ring coordinate arrays.
    Polygon,
    /// An array of Polygon coordinate arrays.
    MultiPolygon,
}
