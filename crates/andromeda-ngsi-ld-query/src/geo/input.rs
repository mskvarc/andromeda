//! Decoded NGSI-LD geo-query input before JSON-LD context expansion.

use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use thiserror::Error;

/// A complete clause-4.10 geo-query with a validated reference geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct GeoQueryInput {
    /// Spatial relationship to evaluate.
    pub georel: GeoRel,
    /// Validated GeoJSON reference geometry.
    pub geometry: Geometry,
    /// Compact GeoProperty identifier, or `location` when absent.
    pub geoproperty: Option<NgsiLdReference>,
}

impl GeoQueryInput {
    /// Construct a complete decoded geo-query.
    pub const fn new(georel: GeoRel, geometry: Geometry, geoproperty: Option<NgsiLdReference>) -> GeoQueryInput {
        GeoQueryInput { georel, geometry, geoproperty }
    }

    /// Decode the three wire components exactly once at request extraction.
    pub fn from_wire(
        georel: String,
        geometry_type: String,
        coordinates: serde_json::Value,
        geoproperty: Option<NgsiLdReference>,
    ) -> Result<GeoQueryInput, GeoQueryInputError> {
        let georel = georel.parse().map_err(GeoQueryInputError::GeoRel)?;
        let coordinates = match coordinates {
            serde_json::Value::String(value) => serde_json::from_str(&value).map_err(GeoQueryInputError::Coordinates)?,
            value => value,
        };
        let value = serde_json::json!({ "type": geometry_type, "coordinates": coordinates });
        let geometry = Geometry::from_geojson_value(&value).map_err(GeoQueryInputError::Geometry)?;
        Ok(GeoQueryInput::new(georel, geometry, geoproperty))
    }
}

/// Geo-query wire decoding failure.
#[derive(Debug, Error)]
pub enum GeoQueryInputError {
    /// The relation is not defined by clause 4.10.
    #[error("invalid georel: {0}")]
    GeoRel(andromeda_ngsi_ld_geometry::geo_rel::GeoRelParseError),
    /// The URL coordinate member was not JSON.
    #[error("invalid geo-query coordinates: {0}")]
    Coordinates(serde_json::Error),
    /// The reference geometry is invalid.
    #[error("invalid geo-query geometry: {0}")]
    Geometry(andromeda_ngsi_ld_geometry::geometry::GeometryError),
}
