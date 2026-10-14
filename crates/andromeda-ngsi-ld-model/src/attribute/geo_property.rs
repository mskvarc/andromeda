//! The `GeoProperty` attribute family (ETSI GS CIM 009 clause 5.2.7).

use crate::attribute::{common_fields::CommonAttributeFields, geo_property_value::GeoPropertyValue};
use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry::Geometry, geometry_members::JsonLdKeywords, written_geometry::WrittenGeometry};
use andromeda_ngsi_ld_lexicon::coerced_term::CoercedTerm;
use cefact_units::UnitCode;
use serde::Serialize;
use serde_with::skip_serializing_none;
use thiserror::Error;

/// NGSI-LD `GeoProperty` representation (ETSI GS CIM 009 clause 5.2.7).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoProperty {
    /// The geometry, held and serialised as `GeoJSON` per RFC 7946, as the
    /// clause 4.7.2 string, or the NGSI-LD Null of a deleted geometry.
    pub value: GeoPropertyValue,
    /// Measurement unit code: clause 4.7.3 treats a `GeoProperty` carrying
    /// further members as an ordinary Property, which has the `unitCode` of
    /// clause 4.5.2.2.
    pub unit_code: Option<UnitCode>,
    /// Optional JSON-LD datatype for `value`.
    pub value_type: Option<CoercedTerm>,
    /// Previous geometry value emitted in notifications when `showChanges` is active.
    pub previous_value: Option<GeoPropertyValue>,
    /// Members shared with every other attribute family.
    #[serde(flatten)]
    pub common: CommonAttributeFields,
}

/// Why one geometry of a compact `GeoProperty` instance is not read as the
/// typed [`Geometry`].
#[derive(Debug, Error)]
pub enum TypedGeometryRefusal {
    /// The geometry object carries a JSON-LD keyword, which is no `GeoJSON`
    /// member.
    #[error("the geometry carries a JSON-LD keyword, which is no GeoJSON member")]
    JsonLdKeyword,
    /// The value is no geometry the typed model reads.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

/// Read one written geometry of a `GeoProperty` instance as the typed
/// [`Geometry`].
///
/// The typed geometry keeps a geometry's positions, its `bbox` and its
/// foreign members (IETF RFC 7946 sections 5 and 6.1), so the instance renders
/// again as it was written (clause 4.6.4). A JSON-LD keyword is no `GeoJSON`
/// member at all but JSON-LD the processor would act on -- an `@context` there
/// is one clause 5.5.7 refuses -- so a geometry carrying one is refused.
///
/// # Errors
///
/// Returns [`TypedGeometryRefusal::JsonLdKeyword`] for a geometry carrying a
/// JSON-LD keyword, and the [`GeometryError`] of one clause 4.7.1 refuses.
pub(crate) fn typed_geometry(written: WrittenGeometry) -> Result<Geometry, TypedGeometryRefusal> {
    match written.json_ld_keywords() {
        JsonLdKeywords::Present => Err(TypedGeometryRefusal::JsonLdKeyword),
        JsonLdKeywords::Absent => Ok(written.into_geometry()?),
    }
}
