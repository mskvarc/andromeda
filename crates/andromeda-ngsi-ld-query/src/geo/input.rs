//! Decoded NGSI-LD geo-query input before JSON-LD context expansion.

use andromeda_ngsi_ld_geometry::{
    geo_rel::{GeoRel, GeoRelParseError},
    geojson::GeometryError,
    geometry::Geometry,
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    ser::{Error as SerializeError, SerializeStruct},
};
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

/// The spec-facing members used to assemble one validated geo-query.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GeoQueryInputWire {
    /// Spatial relationship.
    georel: GeoRel,
    /// GeoJSON geometry type.
    geometry: String,
    /// GeoJSON coordinates.
    coordinates: serde_json::Value,
    /// Optional GeoProperty reference.
    geoproperty: Option<NgsiLdReference>,
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

// A derive cannot combine the `geometry` and `coordinates` wire members into
// the single validated `Geometry` held by the domain type.
impl<'de> Deserialize<'de> for GeoQueryInput {
    fn deserialize<D>(deserializer: D) -> Result<GeoQueryInput, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = GeoQueryInputWire::deserialize(deserializer)?;
        GeoQueryInput::from_wire(wire.georel.to_string(), wire.geometry, wire.coordinates, wire.geoproperty).map_err(serde::de::Error::custom)
    }
}

// The domain invariant stores one validated geometry, while Table 5.2.13-1
// exposes its RFC 7946 type and coordinates as two sibling members.
impl Serialize for GeoQueryInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let geometry = serde_json::to_value(&self.geometry).map_err(S::Error::custom)?;
        let object = geometry
            .as_object()
            .ok_or_else(|| S::Error::custom("validated geometry did not serialize as an object"))?;
        let geometry_type = object
            .get("type")
            .ok_or_else(|| S::Error::custom("validated geometry serialized without a type"))?;
        let coordinates = object
            .get("coordinates")
            .ok_or_else(|| S::Error::custom("validated geometry serialized without coordinates"))?;
        let mut state = serializer.serialize_struct("GeoQuery", if self.geoproperty.is_some() { 4 } else { 3 })?;
        state.serialize_field("georel", &self.georel)?;
        state.serialize_field("geometry", geometry_type)?;
        state.serialize_field("coordinates", coordinates)?;
        if let Some(geoproperty) = &self.geoproperty {
            state.serialize_field("geoproperty", geoproperty)?;
        }
        state.end()
    }
}

/// Geo-query wire decoding failure.
#[derive(Debug, Error)]
pub enum GeoQueryInputError {
    /// The relation is not defined by clause 4.10.
    #[error("invalid georel: {0}")]
    GeoRel(GeoRelParseError),
    /// The URL coordinate member was not JSON.
    #[error("invalid geo-query coordinates: {0}")]
    Coordinates(serde_json::Error),
    /// The reference geometry is invalid.
    ///
    /// Carries the geometry diagnostic verbatim so that a position with an
    /// ordinate count RFC 7946 section 3.1.1 gives no meaning to reaches the
    /// client as a `BadRequestData` detail naming that count, rather than as a
    /// bare "invalid geometry".
    #[error("geo-query reference geometry: {0}")]
    Geometry(GeometryError),
}
