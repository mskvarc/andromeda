//! The compiled form of an NGSI-LD geo query (clause 4.10).

use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

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
    /// The GeoProperty to query against, named by its fully expanded IRI.
    ///
    /// Clause 4.10 lets `geoproperty` name any GeoProperty of the entity, and a
    /// GeoProperty is an attribute, so the target is the attribute name that
    /// keys it (clause 5.2.4) rather than a bare IRI. Holding it as such is
    /// what lets the target be looked up in the attribute-name dictionary
    /// without re-parsing it at every use.
    pub geoproperty: AttributeName,
}
