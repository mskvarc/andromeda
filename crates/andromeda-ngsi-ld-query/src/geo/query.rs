//! The compiled form of an NGSI-LD geo query (clause 4.10).

use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
use iri_rs::IriBuf;

/// A compiled geo-spatial query, per NGSI-LD 4.10.
///
/// The `geometry` and `coordinates` parameters arrive as two separate strings
/// and denote one GeoJSON geometry between them, so they are joined and parsed
/// during compilation. Everything downstream receives a geometry that is known
/// to be well formed, and a malformed one is a 400 from the request rather than
/// a failure part-way through building SQL.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoQuery {
    /// The spatial relationship to evaluate.
    pub georel: GeoRel,
    /// The reference geometry the relation is evaluated against.
    pub geometry: Geometry,
    /// The fully expanded IRI of the GeoProperty to query against.
    pub geoproperty_iri: IriBuf,
}
